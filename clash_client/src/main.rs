use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use eframe::{App, egui};
use egui::{Color32, RichText};
use font_kit::source::SystemSource;
use humansize::{BINARY, format_size};
use serde::Deserialize;

#[derive(Deserialize, Default)]
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
}

impl Default for NetworkStats {
    fn default() -> Self {
        Self {
            upload_speed: 0,
            download_speed: 0,
            previous_upload: 0,
            previous_download: 0,
            last_update: std::time::Instant::now(),
        }
    }
}

struct ClashApp {
    clash_process: Option<Child>,
    config_path: String,
    clash_path: String,
    is_running: bool,
    stats: Arc<Mutex<NetworkStats>>,
}

impl ClashApp {
    fn configure_fonts_and_style(ctx: &egui::Context) {
        // 加载自定义字体
        let mut fonts = egui::FontDefinitions::default();
        // 可以在这里添加自定义字体
        let system_source = SystemSource::new();
        for font_name in [
            "Noto Sans CJK SC".to_string(),
            "Noto Sans CJK TC".to_string(),
            "SauceCodeProNerdFont".to_string(),
            "DejaVuSansMonoNerdFont".to_string(),
            "JetBrainsMonoNerdFont".to_string(),
        ] {
            let font_handle = system_source.select_best_match(
                &[font_kit::family_name::FamilyName::Title(font_name.clone())],
                &font_kit::properties::Properties::new(),
            );
            if font_handle.is_err() {
                continue;
            }
            let font = font_handle.unwrap().load();
            if font.is_err() {
                continue;
            }
            let font_data = font.unwrap().copy_font_data();
            if font_data.is_none() {
                continue;
            }
            fonts.font_data.insert(
                font_name.clone(),
                egui::FontData::from_owned(font_data.unwrap().to_vec()).into(),
            );
            fonts
                .families
                .get_mut(&egui::FontFamily::Proportional)
                .unwrap()
                .insert(0, font_name.clone());
            fonts
                .families
                .get_mut(&egui::FontFamily::Monospace)
                .unwrap()
                .insert(0, font_name);
        }

        // 应用字体
        ctx.set_fonts(fonts);

        // 设置样式
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

        let app = Self {
            clash_process: None,
            config_path: "/home/yj/.config/clash-egui/config.yaml".to_string(),
            clash_path: "clash".to_string(), // 假设clash可执行文件在PATH中
            is_running: false,
            stats,
        };

        // 启动监控线程
        thread::spawn(move || {
            loop {
                thread::sleep(Duration::from_millis(1000));

                // 通过HTTP API获取流量信息
                if let Some(traffic) = get_traffic() {
                    let mut stats = stats_clone.lock().unwrap();
                    let elapsed = stats.last_update.elapsed().as_secs() as u64;
                    if elapsed > 0 {
                        stats.upload_speed = (traffic.up - stats.previous_upload) / elapsed;
                        stats.download_speed = (traffic.down - stats.previous_download) / elapsed;
                        stats.previous_upload = traffic.up;
                        stats.previous_download = traffic.down;
                        stats.last_update = std::time::Instant::now();
                    }
                }
            }
        });

        app
    }

    fn start_clash(&mut self) {
        if self.is_running {
            return;
        }

        match Command::new(&self.clash_path)
            .arg("-f")
            .arg(&self.config_path)
            .spawn()
        {
            Ok(child) => {
                self.clash_process = Some(child);
                self.is_running = true;
                println!("Clash started");

                // 初始化流量统计
                if let Some(traffic) = get_traffic() {
                    let mut stats = self.stats.lock().unwrap();
                    stats.previous_upload = traffic.up;
                    stats.previous_download = traffic.down;
                    stats.last_update = std::time::Instant::now();
                }
            }
            Err(e) => {
                eprintln!("Failed to start Clash: {}", e);
            }
        }
    }

    fn stop_clash(&mut self) {
        if let Some(mut child) = self.clash_process.take() {
            match child.kill() {
                Ok(_) => {
                    println!("Clash stopped");
                    self.is_running = false;

                    // 重置网速显示
                    let mut stats = self.stats.lock().unwrap();
                    stats.upload_speed = 0;
                    stats.download_speed = 0;
                }
                Err(e) => {
                    eprintln!("Failed to stop Clash: {}", e);
                    // 尝试恢复进程
                    self.clash_process = Some(child);
                }
            }
        }
    }
}

// 将获取流量的功能移到外部函数
fn get_traffic() -> Option<TrafficInfo> {
    // 通常Clash API在9090端口
    let client = reqwest::blocking::Client::new();
    match client
        .get("http://127.0.0.1:37381/traffic")
        .header("Content-Type", "application/json")
        .send()
    {
        Ok(response) => match response.json::<TrafficInfo>() {
            Ok(traffic) => Some(traffic),
            Err(e) => {
                eprintln!("Failed to parse traffic info: {}", e);
                None
            }
        },
        Err(e) => {
            eprintln!("Failed to get traffic info: {}", e);
            None
        }
    }
}

impl App for ClashApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Clash Client");
            ui.add_space(20.0);

            // 路径设置
            ui.horizontal(|ui| {
                ui.label("Clash 路径:");
                ui.text_edit_singleline(&mut self.clash_path);
            });

            ui.horizontal(|ui| {
                ui.label("配置文件路径:");
                ui.text_edit_singleline(&mut self.config_path);
            });

            ui.add_space(20.0);

            // 启动/停止按钮
            if self.is_running {
                if ui.button("停止 Clash").clicked() {
                    self.stop_clash();
                }
            } else {
                if ui.button("启动 Clash").clicked() {
                    self.start_clash();
                }
            }

            ui.add_space(20.0);

            // 状态显示
            let status_text = if self.is_running {
                RichText::new("运行中").color(Color32::GREEN)
            } else {
                RichText::new("已停止").color(Color32::RED)
            };
            ui.label(status_text);

            ui.add_space(10.0);

            // 获取当前网速信息并显示
            let stats = self.stats.lock().unwrap();

            // 网速显示
            ui.horizontal(|ui| {
                ui.label("上传速度:");
                ui.label(format_size(stats.upload_speed, BINARY));
                ui.label("/s");
            });

            ui.horizontal(|ui| {
                ui.label("下载速度:");
                ui.label(format_size(stats.download_speed, BINARY));
                ui.label("/s");
            });

            // 自动刷新UI
            ctx.request_repaint_after(Duration::from_millis(500));
        });
    }
}

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_min_inner_size([400.0, 300.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Clash Client",
        options,
        Box::new(|cc| Ok(Box::new(ClashApp::new(cc)))),
    )
}
