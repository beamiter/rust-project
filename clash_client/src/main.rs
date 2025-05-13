// Cargo.toml 添加
// tokio = { version = "1", features = ["rt-multi-thread"] }
// once_cell = "1.8"
// eframe = "0.27.2"
// egui = "0.27.2"
// font-kit = "0.11"
// humansize = "2.1"
// serde = { version = "1.0", features = ["derive"] }
// serde_json = "1.0"
// reqwest = { version = "0.11", features = ["json"] }
// serde_yaml = "0.9" // 新增

use std::fs::File; // 新增
use std::io::BufReader;
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration; // 新增

use eframe::{App, egui};
use egui::{Color32, RichText};
use font_kit::source::SystemSource;
use humansize::{BINARY, format_size};
use once_cell::sync::Lazy;
use serde::Deserialize;
use tokio::runtime::Runtime;

static TOKIO_RUNTIME: Lazy<Runtime> =
    Lazy::new(|| Runtime::new().expect("Failed to create Tokio runtime"));

#[derive(Deserialize, Default, Debug)]
struct TrafficInfo {
    up: u64,
    down: u64,
}

// 用于解析 Clash config.yaml 的结构体
#[derive(Deserialize, Debug)]
struct ClashConfigYaml {
    #[serde(rename = "external-controller")]
    external_controller: Option<String>,
    // 如果需要解析其他字段，可以在这里添加
    // mixed-port: Option<u16>,
    // mode: Option<String>,
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

struct ClashApp {
    clash_process: Option<Child>,
    config_path: String,
    clash_path: String,
    is_running: bool,
    stats: Arc<Mutex<NetworkStats>>,
    api_port: String,
}

// 新增函数：从 Clash 配置文件加载 API 端口
fn load_api_port_from_config(config_path: &str) -> Option<String> {
    let file = File::open(config_path).ok()?;
    let reader = BufReader::new(file);
    let config: ClashConfigYaml = serde_yaml::from_reader(reader).ok()?;

    if let Some(controller_addr) = config.external_controller {
        // external-controller 通常是 "host:port" 或 ":port" 格式
        // 我们只关心端口部分
        if let Some(port_str) = controller_addr.split(':').last() {
            if !port_str.is_empty() {
                // 简单验证一下是否是数字，更严格的验证可以添加
                if port_str.chars().all(char::is_numeric) {
                    return Some(port_str.to_string());
                } else {
                    eprintln!(
                        "Warning: Parsed port '{}' from external-controller is not numeric.",
                        port_str
                    );
                }
            } else if controller_addr.starts_with(':') && controller_addr.len() > 1 {
                // 处理 ":port" 的情况
                let port_only_str = &controller_addr[1..];
                if port_only_str.chars().all(char::is_numeric) {
                    return Some(port_only_str.to_string());
                } else {
                    eprintln!(
                        "Warning: Parsed port '{}' from external-controller is not numeric.",
                        port_only_str
                    );
                }
            }
        }
    }
    None
}

impl ClashApp {
    fn configure_fonts_and_style(ctx: &egui::Context) {
        // 加载自定义字体
        let mut fonts = egui::FontDefinitions::default();
        let system_source = SystemSource::new();
        for font_name in [
            "Noto Sans CJK SC".to_string(),
            "Noto Sans CJK TC".to_string(),
            "SauceCodeProNerdFont".to_string(),
            "DejaVuSansMonoNerdFont".to_string(),
            "JetBrainsMonoNerdFont".to_string(),
            "WenQuanYi Micro Hei".to_string(), // 添加一个更常见的 Linux 中文字体
            "Microsoft YaHei".to_string(),     // Windows 上的常见中文字体
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
                Err(_) => {
                    eprintln!("Font {} not found or failed to load.", font_name);
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
        let stats = Arc::new(Mutex::new(NetworkStats::default()));
        let stats_clone = Arc::clone(&stats);

        // 默认配置文件路径和 Clash 可执行文件路径
        // 尝试从常见的用户配置目录获取路径
        let default_config_path = dirs::config_dir()
            .map(|p| p.join("config.yaml")) // 这是一个常见的路径
            .unwrap_or_else(|| std::path::PathBuf::from("/home/yj/config.yaml")) // 回退路径
            .to_string_lossy()
            .to_string();

        // 尝试从配置文件加载 API 端口
        let initial_api_port =
            load_api_port_from_config(&default_config_path).unwrap_or_else(|| {
                println!(
                    "Could not parse API port from '{}', using default '9090'.",
                    default_config_path
                );
                "9090".to_string() // Clash 默认 API 端口
            });

        let app = Self {
            clash_process: None,
            config_path: default_config_path, // 使用上面确定的路径
            clash_path: "clash".to_string(),  // 假设 clash 在 PATH 中
            is_running: false,
            stats,
            api_port: initial_api_port, // 使用从配置加载或默认的端口
        };

        // 监控线程使用的 api_port 应该是 ClashApp 实例中的值
        // 这里克隆的是初始化后的 app.api_port
        let api_port_for_thread = app.api_port.clone();

        // 启动监控线程
        thread::spawn(move || {
            loop {
                thread::sleep(Duration::from_millis(1000));

                // 尝试获取流量数据
                if let Some(traffic) = get_traffic("127.0.0.1", &api_port_for_thread) {
                    println!("traffic: {:?}", traffic); // 调试时可以取消注释
                    let mut stats = stats_clone.lock().unwrap();
                    stats.api_connected = true;
                    let elapsed_secs = stats.last_update.elapsed().as_secs_f64();
                    if elapsed_secs > 0.1 {
                        // 避免除以过小的时间间隔
                        let elapsed_u64 = elapsed_secs as u64; // 转为 u64 用于计算（会损失精度）
                        if elapsed_u64 > 0 {
                            // 确保转换后仍然大于0
                            stats.upload_speed =
                                (traffic.up.saturating_sub(stats.previous_upload)) / elapsed_u64;
                            stats.download_speed =
                                (traffic.down.saturating_sub(stats.previous_download))
                                    / elapsed_u64;
                        } else {
                            // 如果时间间隔太短，近似计算
                            stats.upload_speed =
                                ((traffic.up.saturating_sub(stats.previous_upload)) as f64
                                    / elapsed_secs) as u64;
                            stats.download_speed =
                                ((traffic.down.saturating_sub(stats.previous_download)) as f64
                                    / elapsed_secs) as u64;
                        }
                        stats.previous_upload = traffic.up;
                        stats.previous_download = traffic.down;
                        stats.last_update = std::time::Instant::now();
                    }
                } else {
                    // API连接失败
                    println!("failed to get traffic"); // 调试时可以取消注释
                    let mut stats = stats_clone.lock().unwrap();
                    stats.api_connected = false;
                }
            }
        });

        app
    }

    fn start_clash(&mut self) {
        if self.is_running {
            return;
        }

        // 在启动 Clash 前，尝试从当前 config_path 重新加载 API 端口
        // 这样即使用户在 UI 修改了 config_path，也能尝试使用新配置的端口
        if let Some(parsed_port) = load_api_port_from_config(&self.config_path) {
            if self.api_port != parsed_port {
                println!(
                    "API port updated to '{}' from config file '{}' before starting Clash.",
                    parsed_port, self.config_path
                );
                self.api_port = parsed_port;
                // 注意：这里的修改不会影响已经启动的监控线程，监控线程仍然使用启动时捕获的 api_port。
                // 如果需要动态更新监控线程的端口，则需要更复杂的线程通信机制。
                // 但对于启动Clash这个场景，确保Clash启动时使用的是配置文件里的API端口是合理的。
            }
        }

        match Command::new(&self.clash_path)
            .arg("-f")
            .arg(&self.config_path)
            .spawn()
        {
            Ok(child) => {
                self.clash_process = Some(child);
                self.is_running = true;
                println!("Clash started with config: {}", self.config_path);
            }
            Err(e) => {
                eprintln!(
                    "Failed to start Clash (path: '{}', config: '{}'): {}",
                    self.clash_path, self.config_path, e
                );
            }
        }
    }

    fn stop_clash(&mut self) {
        if let Some(mut child) = self.clash_process.take() {
            match child.kill() {
                Ok(_) => {
                    // 等待进程实际退出，可选
                    // match child.wait() {
                    //     Ok(status) => println!("Clash process exited with status: {}", status),
                    //     Err(e) => eprintln!("Error waiting for Clash process to exit: {}", e),
                    // }
                    println!("Clash stopped");
                    self.is_running = false;

                    let mut stats = self.stats.lock().unwrap();
                    stats.upload_speed = 0;
                    stats.download_speed = 0;
                    // API 连接状态也应该在停止时更新，因为 Clash 停了 API 自然就不可用了
                    // 但监控线程会自行检测并更新 api_connected，所以这里不强制设为 false
                }
                Err(e) => {
                    eprintln!("Failed to stop Clash: {}", e);
                    self.clash_process = Some(child); // 恢复进程句柄
                }
            }
        }
    }
}

async fn get_traffic_async(host: &str, port: &str) -> Option<TrafficInfo> {
    let base_url = format!("http://{}:{}", host, port);
    println!("{base_url}");
    // Clash API /traffic 端点通常更直接地提供总流量
    // 尝试 /traffic 端点
    if let Some(traffic) = try_traffic_endpoint(&base_url).await {
        return Some(traffic);
    }
    // 如果 /traffic 失败，回退到 /connections (但/connections通常是实时连接，不是总流量)
    // 注意：Clash 的 /connections 端点返回的是当前活跃连接的列表及其各自的流量，
    // 而不是总的累计流量。总累计流量通常由 /traffic 端点提供。
    // 你之前的实现是从 /connections 累加，这可能不是你想要的总流量。
    // 如果确实需要从 /connections 获取，那么逻辑是正确的，但请注意其含义。
    // 此处我保留了 /connections 的逻辑，但优先尝试 /traffic。
    if let Some(traffic) = try_connections_endpoint(&base_url).await {
        eprintln!(
            "Warning: Using /connections endpoint for traffic. This might not be total accumulated traffic."
        );
        return Some(traffic);
    }
    None
}

// 新增：尝试从 /traffic 端点获取流量
async fn try_traffic_endpoint(base_url: &str) -> Option<TrafficInfo> {
    println!("haha 0");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5)) // 短超时
        .build()
        .ok()?;
    println!("haha 1");

    let response = client
        .get(&format!("{}/traffic", base_url))
        .send()
        .await
        .ok()?;
    println!("haha 2");

    if !response.status().is_success() {
        eprintln!("Failed to get /traffic: HTTP {}", response.status());
        return None;
    }
    println!("haha 3");

    // /traffic 端点返回的是一个字节流，格式是 up: N\ndown: N\n
    // 我们需要逐行解析
    let body = response.text().await.ok()?;
    let mut up = 0;
    let mut down = 0;

    for line in body.lines() {
        if let Some(val_str) = line.strip_prefix("up: ") {
            up = val_str.parse().unwrap_or(0);
        } else if let Some(val_str) = line.strip_prefix("down: ") {
            down = val_str.parse().unwrap_or(0);
        }
    }
    println!("haha 4");

    if up > 0 || down > 0 {
        // 确保至少解析到一些数据
        println!("Used /traffic endpoint: up={}, down={}", up, down);
        Some(TrafficInfo { up, down })
    } else {
        eprintln!(
            "Failed to parse traffic data from /traffic endpoint body: {}",
            body
        );
        None
    }
}

async fn try_connections_endpoint(base_url: &str) -> Option<TrafficInfo> {
    println!("there 0");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .ok()?;
    println!("there 1");

    let response = client
        .get(&format!("{}/connections", base_url))
        .send()
        .await
        .ok()?;
    println!("there 2");

    if !response.status().is_success() {
        eprintln!("Failed to get /connections: HTTP {}", response.status());
        return None;
    }
    println!("there 3");

    let data = response.json::<serde_json::Value>().await.ok()?;
    let mut up = 0;
    let mut down = 0;

    if let Some(connections) = data["connections"].as_array() {
        for conn in connections {
            // 从 connections 累加的是当前活跃连接的瞬时总流量，而不是历史累计总流量
            up += conn["upload"].as_u64().unwrap_or(0);
            down += conn["download"].as_u64().unwrap_or(0);
        }
    }

    println!("there 4");
    // 只有当通过 /connections 真的获取到了数据才认为成功
    if up > 0
        || down > 0
        || data["connections"]
            .as_array()
            .map_or(false, |a| !a.is_empty())
    {
        println!("Used /connections endpoint: up={}, down={}", up, down);
        Some(TrafficInfo { up, down })
    } else {
        eprintln!("No traffic data parsed from /connections endpoint.");
        None
    }
}

fn get_traffic(host: &str, port: &str) -> Option<TrafficInfo> {
    TOKIO_RUNTIME.block_on(get_traffic_async(host, port))
}

impl App for ClashApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Clash 控制面板"); // 本地化一点
            ui.add_space(20.0);

            ui.collapsing("⚙️ 配置设置", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Clash 可执行文件路径:");
                    ui.text_edit_singleline(&mut self.clash_path);
                });

                ui.horizontal(|ui| {
                    ui.label("Clash 配置文件路径:");
                    if ui.text_edit_singleline(&mut self.config_path).changed() {
                        // 如果配置文件路径改变了，尝试重新加载 API 端口
                        if let Some(parsed_port) = load_api_port_from_config(&self.config_path) {
                            if self.api_port != parsed_port {
                                println!("API port updated to '{}' from config file '{}' due to UI change.", parsed_port, self.config_path);
                                self.api_port = parsed_port;
                                // 再次提醒：监控线程的端口不会因此改变。
                            }
                        } else {
                             println!("Failed to parse API port from new config path: '{}'. Keeping current API port: '{}'", self.config_path, self.api_port);
                        }
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("Clash API 端口:");
                    ui.text_edit_singleline(&mut self.api_port);
                    // 注意：用户在这里修改 API 端口后，如果与配置文件不一致，
                    // 下次启动 Clash (self.start_clash()) 时，
                    // 如果配置文件中存在 external-controller，则会优先使用配置文件中的端口。
                    // 监控线程使用的 API 端口是在应用启动时确定的，不会因 UI 修改而改变。
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

            let stats = self.stats.lock().unwrap();

            ui.horizontal(|ui| {
                let status_text = if self.is_running {
                    RichText::new("🟢 Clash 运行中").color(Color32::GREEN)
                } else {
                    RichText::new("🔴 Clash 已停止").color(Color32::RED)
                };
                ui.label(status_text);

                ui.separator();

                let api_text = if stats.api_connected {
                    RichText::new("🔗 API 已连接").color(Color32::GREEN)
                } else {
                    RichText::new("⚠️ API 未连接").color(Color32::RED)
                };
                ui.label(api_text);
            });

            ui.add_space(10.0);

            ui.horizontal(|ui| {
                ui.label(format!("⬆️ 上传: {}/s", format_size(stats.upload_speed, BINARY)));
                ui.separator();
                ui.label(format!("⬇️ 下载: {}/s", format_size(stats.download_speed, BINARY)));
            });
             ui.horizontal(|ui| {
                ui.label(format!("总上传: {}", format_size(stats.previous_upload, BINARY)));
                ui.separator();
                ui.label(format!("总下载: {}", format_size(stats.previous_download, BINARY)));
            });


            ctx.request_repaint_after(Duration::from_millis(500));
        });
    }
}

fn main() -> Result<(), eframe::Error> {
    // 尝试使用 dirs crate 获取用户配置目录，更健壮
    // 需要添加 dirs = "5.0" 到 Cargo.toml
    // 在 main 函数顶部: use dirs;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([600.0, 400.0]) // 调整了默认大小
            .with_min_inner_size([400.0, 300.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Clash 控制面板", // 本地化标题
        options,
        Box::new(|cc| Ok(Box::new(ClashApp::new(cc)))),
    )
}
