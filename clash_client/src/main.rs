use eframe::{App, Frame, egui};
use egui::{Align, Color32, Layout, RichText, ScrollArea, Ui};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::fs;
use std::process::{Child, Command};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use tokio::sync::{Semaphore, mpsc};

// --- 模块划分 ---

mod models {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Deserialize, Serialize, Debug, Default, Clone)]
    #[serde(rename_all = "kebab-case")]
    pub struct ClashFullConfig {
        #[serde(rename = "mixed-port")]
        pub mixed_port: Option<u16>,
        #[serde(rename = "external-controller")]
        pub external_controller: Option<String>,
        #[serde(default)]
        pub proxies: Vec<ProxyConfigEntry>,
    }

    #[derive(Debug, PartialEq, Clone, Copy)]
    pub enum CentralView {
        AllProxies,
        ConfigEditor,
    }

    #[derive(Debug, PartialEq, Clone, Copy)]
    pub enum ProxySortBy {
        Name,
        Latency,
    }

    #[derive(Debug, Clone)]
    pub struct ProxyLatencyResult {
        pub proxy_name: String,
        pub latency_ms: Option<u32>,
        pub status_message: String,
    }

    #[derive(Deserialize, Serialize, Debug, Default, Clone)]
    #[serde(rename_all = "kebab-case")]
    pub struct ProxyDetail {
        pub name: String,
        #[serde(rename = "type")]
        pub proxy_type: String,
        pub server: String,
        pub port: u16,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ProxyConfigEntry {
        #[serde(flatten)]
        pub details: ProxyDetail,
        #[serde(skip)]
        pub latency_ms: Option<u32>,
        #[serde(skip)]
        pub latency_test_status: String,
    }

    impl Default for ProxyConfigEntry {
        fn default() -> Self {
            Self {
                details: ProxyDetail::default(),
                latency_ms: None,
                latency_test_status: "N/A".to_string(),
            }
        }
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct ClashApiGeneralConfig {
        pub mode: String,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct ClashApiSelectorProxyInfo {
        pub now: Option<String>,
    }

    #[derive(Deserialize, Debug, Clone)]
    pub struct ClashApiDelayResponse {
        pub delay: Option<u32>,
        pub message: Option<String>,
    }

    #[derive(Debug, Clone)]
    pub struct AppDynamicClashInfo {
        pub mode: String,
        pub current_global_proxy_name: Option<String>,
        pub current_global_proxy_latency: String,
    }

    impl Default for AppDynamicClashInfo {
        fn default() -> Self {
            Self {
                mode: "未知".to_string(),
                current_global_proxy_name: None,
                current_global_proxy_latency: "N/A".to_string(),
            }
        }
    }

    #[derive(Debug)]
    pub struct NetworkStats {
        pub upload_speed: u64,
        pub download_speed: u64,
        pub previous_upload: u64,
        pub previous_download: u64,
        pub last_update: Instant,
        pub api_connected: bool,
    }

    impl Default for NetworkStats {
        fn default() -> Self {
            Self {
                upload_speed: 0,
                download_speed: 0,
                previous_upload: 0,
                previous_download: 0,
                last_update: Instant::now(),
                api_connected: false,
            }
        }
    }

    #[derive(Deserialize, Serialize, Debug, Clone)]
    #[serde(default)]
    pub struct AppState {
        pub config_path: String,
        pub clash_path: String,
        pub api_port: String,
        pub latency_test_url: String,
        pub latency_test_timeout_ms: u32,
    }

    impl Default for AppState {
        fn default() -> Self {
            let default_config_path = dirs::config_dir()
                .map(|p| p.join("clash/config.yaml"))
                .unwrap_or_else(|| std::path::PathBuf::from("config.yaml"))
                .to_string_lossy()
                .to_string();

            Self {
                config_path: default_config_path,
                clash_path: "clash".to_string(),
                api_port: "9090".to_string(),
                latency_test_url: "http://www.gstatic.com/generate_204".to_string(),
                latency_test_timeout_ms: 5000,
            }
        }
    }
}

use models::*;

// --- 全局运行时 ---
static TOKIO_RUNTIME: Lazy<Runtime> =
    Lazy::new(|| Runtime::new().expect("Failed to create Tokio runtime"));

// --- 工具函数 ---
fn extract_port_from_controller_string(controller_addr: &str) -> Option<String> {
    if let Some(port_str) = controller_addr.split(':').last() {
        if !port_str.is_empty() && port_str.chars().all(char::is_numeric) {
            return Some(port_str.to_string());
        }
    }
    None
}

// --- 核心应用逻辑 ---

struct ClashApp {
    app_state: AppState,
    is_running: bool,
    clash_process: Option<Child>,

    config_content: String,
    config_editor_status: String,

    stats: Arc<RwLock<NetworkStats>>,
    dynamic_clash_info: Arc<RwLock<AppDynamicClashInfo>>,
    all_proxies_for_ui: Arc<RwLock<Vec<ProxyConfigEntry>>>,

    api_port_for_monitor: Arc<RwLock<String>>,
    is_testing_all_proxies: Arc<Mutex<bool>>,
    _is_testing_latency: Arc<Mutex<bool>>,

    all_proxies_test_status: String,
    proxy_sort_by: ProxySortBy,
    current_central_view: CentralView,

    latency_result_receiver: mpsc::Receiver<ProxyLatencyResult>,
    latency_result_sender: mpsc::Sender<ProxyLatencyResult>,
}

impl ClashApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self::configure_fonts(&cc.egui_ctx);

        let app_state: AppState = if let Some(storage) = cc.storage {
            eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default()
        } else {
            AppState::default()
        };

        let stats = Arc::new(RwLock::new(NetworkStats::default()));
        let dynamic_clash_info = Arc::new(RwLock::new(AppDynamicClashInfo::default()));
        let all_proxies_for_ui = Arc::new(RwLock::new(Vec::new()));
        let api_port_for_monitor = Arc::new(RwLock::new(app_state.api_port.clone()));

        let (tx, rx) = mpsc::channel(100);

        let mut app = Self {
            app_state,
            is_running: false,
            clash_process: None,
            config_content: String::new(),
            config_editor_status: String::new(),
            stats: stats.clone(),
            dynamic_clash_info: dynamic_clash_info.clone(),
            all_proxies_for_ui: all_proxies_for_ui.clone(),
            api_port_for_monitor: api_port_for_monitor.clone(),
            is_testing_all_proxies: Arc::new(Mutex::new(false)),
            _is_testing_latency: Arc::new(Mutex::new(false)),
            all_proxies_test_status: "未开始".to_string(),
            proxy_sort_by: ProxySortBy::Name,
            current_central_view: CentralView::AllProxies,
            latency_result_receiver: rx,
            latency_result_sender: tx,
        };

        app.load_config_from_file();

        let monitor_port = api_port_for_monitor.clone();
        let monitor_stats = stats.clone();
        let monitor_info = dynamic_clash_info.clone();

        TOKIO_RUNTIME.spawn(async move {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(2))
                .build()
                .unwrap();

            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;

                let port = { monitor_port.read().unwrap().clone() };
                if port.is_empty() {
                    let mut s = monitor_stats.write().unwrap();
                    s.api_connected = false;
                    continue;
                }

                let base_url = format!("http://127.0.0.1:{}", port);
                Self::update_traffic(&client, &base_url, &monitor_stats).await;
                Self::update_clash_info(&client, &base_url, &monitor_info).await;
            }
        });

        app
    }

    // --- 修复：跨平台字体加载 ---
    fn configure_fonts(ctx: &egui::Context) {
        let mut fonts = egui::FontDefinitions::default();
        let system_source = font_kit::source::SystemSource::new();

        // 尝试加载的中文字体列表（涵盖 Linux/Windows/macOS）
        let font_families = [
            "Noto Sans CJK SC",
            "WenQuanYi Micro Hei", // Linux 常见
            "Microsoft YaHei",     // Windows
            "PingFang SC",         // macOS
            "SimHei",
            "Arial Unicode MS",
        ];

        for font_name in font_families {
            if let Ok(handle) = system_source.select_best_match(
                &[font_kit::family_name::FamilyName::Title(
                    font_name.to_string(),
                )],
                &font_kit::properties::Properties::new(),
            ) {
                if let Ok(font) = handle.load() {
                    if let Some(font_data) = font.copy_font_data() {
                        println!("Loaded system font: {}", font_name);
                        fonts.font_data.insert(
                            "my_font".to_owned(),
                            egui::FontData::from_owned(font_data.to_vec()).into(),
                        );
                        // 设置为首选字体
                        fonts
                            .families
                            .entry(egui::FontFamily::Proportional)
                            .or_default()
                            .insert(0, "my_font".to_owned());
                        fonts
                            .families
                            .entry(egui::FontFamily::Monospace)
                            .or_default()
                            .insert(0, "my_font".to_owned());
                        break; // 找到一个就退出
                    }
                }
            }
        }
        // 如果没找到中文字体，egui 将回退到默认字体（中文可能显示方框），但不会崩溃
        ctx.set_fonts(fonts);
    }

    async fn update_traffic(
        client: &reqwest::Client,
        base_url: &str,
        stats: &Arc<RwLock<NetworkStats>>,
    ) {
        let url = format!("{}/connections", base_url);
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(data) = resp.json::<serde_json::Value>().await {
                    let mut up = 0;
                    let mut down = 0;
                    if let Some(conns) = data["connections"].as_array() {
                        for c in conns {
                            up += c["upload"].as_u64().unwrap_or(0);
                            down += c["download"].as_u64().unwrap_or(0);
                        }
                    }

                    let mut s = stats.write().unwrap();
                    s.api_connected = true;
                    let now = Instant::now();
                    let elapsed = now.duration_since(s.last_update).as_secs_f64();
                    if elapsed >= 0.1 {
                        s.upload_speed =
                            ((up.saturating_sub(s.previous_upload)) as f64 / elapsed) as u64;
                        s.download_speed =
                            ((down.saturating_sub(s.previous_download)) as f64 / elapsed) as u64;
                        s.previous_upload = up;
                        s.previous_download = down;
                        s.last_update = now;
                    }
                }
            }
            _ => {
                let mut s = stats.write().unwrap();
                s.api_connected = false;
                s.upload_speed = 0;
                s.download_speed = 0;
            }
        }
    }

    async fn update_clash_info(
        client: &reqwest::Client,
        base_url: &str,
        info: &Arc<RwLock<AppDynamicClashInfo>>,
    ) {
        let config_url = format!("{}/configs", base_url);
        let mut new_mode = "获取失败".to_string();

        if let Ok(resp) = client.get(&config_url).send().await {
            if let Ok(cfg) = resp.json::<ClashApiGeneralConfig>().await {
                new_mode = cfg.mode;
            }
        }

        let mut new_global = None;
        if new_mode.to_lowercase() == "global" {
            let proxy_url = format!("{}/proxies/GLOBAL", base_url);
            if let Ok(resp) = client.get(&proxy_url).send().await {
                if let Ok(p) = resp.json::<ClashApiSelectorProxyInfo>().await {
                    new_global = p.now;
                }
            }
        }

        let mut i = info.write().unwrap();
        if i.mode != new_mode || i.current_global_proxy_name != new_global {
            i.current_global_proxy_latency = "N/A".to_string();
        }
        i.mode = new_mode;
        i.current_global_proxy_name = new_global;
    }

    fn load_config_from_file(&mut self) {
        match fs::read_to_string(&self.app_state.config_path) {
            Ok(content) => {
                self.config_content = content.clone();
                match serde_yaml::from_str::<ClashFullConfig>(&content) {
                    Ok(parsed) => {
                        self.config_editor_status =
                            format!("加载成功 ({})", chrono::Local::now().format("%H:%M:%S"));

                        {
                            let mut list = self.all_proxies_for_ui.write().unwrap();
                            *list = parsed.proxies.clone();
                            for p in list.iter_mut() {
                                p.latency_ms = None;
                                p.latency_test_status = "N/A".to_string();
                            }
                        }

                        if let Some(ec) = parsed.external_controller {
                            if let Some(port) = extract_port_from_controller_string(&ec) {
                                if self.app_state.api_port != port {
                                    self.app_state.api_port = port.clone();
                                    *self.api_port_for_monitor.write().unwrap() = port;
                                }
                            }
                        }
                        self.sort_proxies_for_ui();
                    }
                    Err(e) => self.config_editor_status = format!("解析失败: {}", e),
                }
            }
            Err(e) => self.config_editor_status = format!("读取失败: {}", e),
        }
    }

    fn save_config_to_file(&mut self) {
        if let Err(e) = fs::write(&self.app_state.config_path, &self.config_content) {
            self.config_editor_status = format!("保存失败: {}", e);
        } else {
            self.config_editor_status =
                format!("已保存 ({})", chrono::Local::now().format("%H:%M:%S"));
            self.load_config_from_file();
        }
    }

    fn test_all_proxies(&mut self) {
        let mut is_testing = self.is_testing_all_proxies.lock().unwrap();
        if *is_testing {
            return;
        }
        *is_testing = true;
        self.all_proxies_test_status = "准备测试...".to_string();

        let proxies: Vec<ProxyDetail> = self
            .all_proxies_for_ui
            .read()
            .unwrap()
            .iter()
            .map(|p| p.details.clone())
            .collect();

        if proxies.is_empty() {
            self.all_proxies_test_status = "列表为空".to_string();
            *is_testing = false;
            return;
        }

        {
            let mut list = self.all_proxies_for_ui.write().unwrap();
            for p in list.iter_mut() {
                p.latency_test_status = "待测试...".to_string();
                p.latency_ms = None;
            }
        }

        let api_port = self.app_state.api_port.clone();
        let test_url = self.app_state.latency_test_url.clone();
        let timeout = self.app_state.latency_test_timeout_ms;
        let tx = self.latency_result_sender.clone();

        let semaphore = Arc::new(Semaphore::new(50));

        TOKIO_RUNTIME.spawn(async move {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_millis(timeout as u64 + 1000))
                .build()
                .unwrap();

            for proxy in proxies {
                let p_name = proxy.name.clone();
                let port = api_port.clone();
                let t_url = test_url.clone();
                let tx_inner = tx.clone();
                let client_inner = client.clone();
                let sem_clone = semaphore.clone();

                tokio::spawn(async move {
                    let _permit = sem_clone.acquire().await.unwrap();

                    let url_name = urlencoding::encode(&p_name);
                    let url_test = urlencoding::encode(&t_url);
                    let req_url = format!(
                        "http://127.0.0.1:{}/proxies/{}/delay?timeout={}&url={}",
                        port, url_name, timeout, url_test
                    );

                    let (latency, status) = match client_inner.get(&req_url).send().await {
                        Ok(resp) => {
                            if resp.status().is_success() {
                                if let Ok(json) = resp.json::<ClashApiDelayResponse>().await {
                                    if let Some(d) = json.delay {
                                        (Some(d), format!("{} ms", d))
                                    } else {
                                        (None, json.message.unwrap_or_else(|| "超时".into()))
                                    }
                                } else {
                                    (None, "解析错误".into())
                                }
                            } else {
                                (None, "请求失败".into())
                            }
                        }
                        Err(_) => (None, "连接失败".into()),
                    };

                    let _ = tx_inner
                        .send(ProxyLatencyResult {
                            proxy_name: p_name,
                            latency_ms: latency,
                            status_message: status,
                        })
                        .await;
                });
            }
        });
    }

    fn sort_proxies_for_ui(&mut self) {
        let mut list = self.all_proxies_for_ui.write().unwrap();
        match self.proxy_sort_by {
            ProxySortBy::Name => list.sort_by(|a, b| a.details.name.cmp(&b.details.name)),
            ProxySortBy::Latency => list.sort_by(|a, b| match (a.latency_ms, b.latency_ms) {
                (Some(x), Some(y)) => x.cmp(&y),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.details.name.cmp(&b.details.name),
            }),
        }
    }

    fn process_latency_results(&mut self) {
        let mut count = 0;
        let max_per_frame = 20;
        let mut need_sort = false;

        while count < max_per_frame {
            match self.latency_result_receiver.try_recv() {
                Ok(res) => {
                    let mut list = self.all_proxies_for_ui.write().unwrap();
                    if let Some(p) = list.iter_mut().find(|p| p.details.name == res.proxy_name) {
                        p.latency_ms = res.latency_ms;
                        p.latency_test_status = res.status_message;
                        need_sort = self.proxy_sort_by == ProxySortBy::Latency;
                    }
                    count += 1;
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(_) => {
                    *self.is_testing_all_proxies.lock().unwrap() = false;
                    break;
                }
            }
        }

        if need_sort {
            self.sort_proxies_for_ui();
        }

        let is_testing = *self.is_testing_all_proxies.lock().unwrap();
        if is_testing {
            let list = self.all_proxies_for_ui.read().unwrap();
            let all_done = list.iter().all(|p| {
                p.latency_test_status != "待测试..." && p.latency_test_status != "准备测试..."
            });
            if all_done {
                drop(list);
                *self.is_testing_all_proxies.lock().unwrap() = false;
                self.all_proxies_test_status = "测试完成".to_string();
                if self.proxy_sort_by == ProxySortBy::Latency {
                    self.sort_proxies_for_ui();
                }
            } else {
                self.all_proxies_test_status = "正在测试...".to_string();
            }
        }
    }

    fn render_sidebar(&mut self, _ctx: &egui::Context, ui: &mut Ui) {
        ui.heading("Clash 控制面板");
        ui.add_space(10.0);

        ui.label(RichText::new("导航").strong());
        if ui
            .selectable_value(
                &mut self.current_central_view,
                CentralView::AllProxies,
                "🚦 所有节点",
            )
            .clicked()
        {}
        if ui
            .selectable_value(
                &mut self.current_central_view,
                CentralView::ConfigEditor,
                "📄 配置文件",
            )
            .clicked()
        {}
        ui.separator();

        ui.collapsing("ℹ️ 状态监控", |ui| {
            let (mode, global, lat) = {
                let i = self.dynamic_clash_info.read().unwrap();
                (
                    i.mode.clone(),
                    i.current_global_proxy_name.clone(),
                    i.current_global_proxy_latency.clone(),
                )
            };

            ui.horizontal(|ui| {
                ui.label("模式:");
                ui.strong(&mode);
            });
            if mode.to_lowercase() == "global" {
                ui.horizontal(|ui| {
                    ui.label("当前节点:");
                    ui.colored_label(Color32::LIGHT_BLUE, global.unwrap_or("无".into()));
                });
                ui.horizontal(|ui| {
                    ui.label("延迟:");
                    ui.strong(&lat);
                });
            }
        });

        ui.separator();

        // --- 修复：借用检查冲突 ---
        // 先读取数据，释放锁，再渲染可能调用 self 方法的按钮
        use humansize::{BINARY, format_size};
        let (api_connected, up_speed, down_speed) = {
            let stats = self.stats.read().unwrap();
            (
                stats.api_connected,
                stats.upload_speed,
                stats.download_speed,
            )
        };

        ui.horizontal(|ui| {
            ui.label(if self.is_running {
                "🟢 运行中"
            } else {
                "🔴 已停止"
            });
            ui.label(if api_connected {
                "🔗 API联通"
            } else {
                "⚠️ API断开"
            });
        });

        ui.label(format!(
            "⬆️ {}/s  ⬇️ {}/s",
            format_size(up_speed, BINARY),
            format_size(down_speed, BINARY)
        ));

        ui.separator();

        // 此时 stats 锁已释放，可以安全调用 mut self 方法
        if self.is_running {
            if ui.button("⏹️ 停止 Clash").clicked() {
                self.stop_clash();
            }
        } else {
            if ui.button("▶️ 启动 Clash").clicked() {
                self.start_clash();
            }
        }

        ui.separator();
        ui.collapsing("⚙️ 设置", |ui| {
            ui.label("API 端口:");
            if ui
                .text_edit_singleline(&mut self.app_state.api_port)
                .changed()
            {
                *self.api_port_for_monitor.write().unwrap() = self.app_state.api_port.clone();
            }
            ui.label("Clash 路径:");
            ui.text_edit_singleline(&mut self.app_state.clash_path);
            ui.label("Config 路径:");
            ui.text_edit_singleline(&mut self.app_state.config_path);
        });
    }

    fn render_proxy_list(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.heading("节点列表");
            let testing = *self.is_testing_all_proxies.lock().unwrap();
            if ui
                .add_enabled(!testing, egui::Button::new("🧪 测试延迟"))
                .clicked()
            {
                self.test_all_proxies();
            }
            ui.label(&self.all_proxies_test_status);
        });

        ui.horizontal(|ui| {
            ui.label("排序:");
            if ui
                .selectable_value(&mut self.proxy_sort_by, ProxySortBy::Name, "名称")
                .changed()
            {
                self.sort_proxies_for_ui();
            }
            if ui
                .selectable_value(&mut self.proxy_sort_by, ProxySortBy::Latency, "延迟")
                .changed()
            {
                self.sort_proxies_for_ui();
            }
        });

        ui.separator();

        let proxies = self.all_proxies_for_ui.read().unwrap();
        let global_name = self
            .dynamic_clash_info
            .read()
            .unwrap()
            .current_global_proxy_name
            .clone();

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, 24.0, proxies.len(), |ui, range| {
                for entry in &proxies[range] {
                    ui.horizontal(|ui| {
                        let is_active = Some(&entry.details.name) == global_name.as_ref();
                        let name_text = if is_active {
                            RichText::new(&entry.details.name)
                                .color(Color32::LIGHT_GREEN)
                                .strong()
                        } else {
                            RichText::new(&entry.details.name)
                        };

                        ui.label(name_text)
                            .on_hover_text(format!("Server: {}", entry.details.server));

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let color = if entry.latency_ms.is_some() {
                                Color32::GREEN
                            } else {
                                Color32::GRAY
                            };
                            ui.colored_label(color, &entry.latency_test_status);

                            if ui.button("Set").clicked() {
                                self.set_proxy_global(entry.details.name.clone());
                            }
                        });
                    });
                    ui.separator();
                }
            });
    }

    fn render_config_editor(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if ui.button("🔄 重载").clicked() {
                self.load_config_from_file();
            }
            if ui.button("💾 保存").clicked() {
                self.save_config_to_file();
            }
            ui.label(&self.config_editor_status);
        });

        ScrollArea::vertical().show(ui, |ui| {
            ui.add_sized(
                ui.available_size(),
                egui::TextEdit::multiline(&mut self.config_content)
                    .font(egui::TextStyle::Monospace)
                    .code_editor(),
            );
        });
    }

    fn start_clash(&mut self) {
        if self.is_running {
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
            }
            Err(e) => self.config_editor_status = format!("启动失败: {}", e),
        }
    }

    fn stop_clash(&mut self) {
        if let Some(mut child) = self.clash_process.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.is_running = false;
    }

    fn set_proxy_global(&self, name: String) {
        let port = self.app_state.api_port.clone();
        TOKIO_RUNTIME.spawn(async move {
            let client = reqwest::Client::new();
            let url = format!("http://127.0.0.1:{}/proxies/GLOBAL", port);
            let mut map = HashMap::new();
            map.insert("name", name);
            let _ = client.put(url).json(&map).send().await;
        });
    }
}

// --- eframe 实现 ---

impl App for ClashApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.app_state);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        self.process_latency_results();

        egui::SidePanel::left("sidebar")
            .min_width(200.0)
            .show(ctx, |ui| {
                self.render_sidebar(ctx, ui);
            });

        egui::CentralPanel::default().show(ctx, |ui| match self.current_central_view {
            CentralView::AllProxies => self.render_proxy_list(ui),
            CentralView::ConfigEditor => self.render_config_editor(ui),
        });

        let is_testing = *self.is_testing_all_proxies.lock().unwrap();
        if is_testing {
            ctx.request_repaint_after(Duration::from_millis(100));
        } else {
            ctx.request_repaint_after(Duration::from_secs(1));
        }
    }
}

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([900.0, 600.0]),
        ..Default::default()
    };

    // --- 修复：返回值类型 ---
    eframe::run_native(
        "Clash 控制面板 Pro",
        options,
        Box::new(|cc| Ok(Box::new(ClashApp::new(cc)))),
    )
}
