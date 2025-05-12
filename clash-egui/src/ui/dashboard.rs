use crate::clash::core::ClashCore;
use eframe::egui;
use log::info;
use std::sync::{Arc, Mutex};

pub struct Dashboard {
    core: Arc<Mutex<ClashCore>>,
    upload_speed: f64,
    download_speed: f64,
    last_upload: u64,
    last_download: u64,
    last_update: std::time::Instant,
    connection_count: usize,
}

impl Dashboard {
    pub fn new(core: Arc<Mutex<ClashCore>>) -> Self {
        Self {
            core,
            upload_speed: 0.0,
            download_speed: 0.0,
            last_upload: 0,
            last_download: 0,
            last_update: std::time::Instant::now(),
            connection_count: 0,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // info!("Dashboard 0");
        ui.heading("Dashboard");
        ui.add_space(20.0);

        // 更新流量数据
        self.update_traffic_data();
        // info!("Dashboard 1");

        // 显示流量卡片
        ui.horizontal(|ui| {
            self.traffic_card(
                ui,
                "upload_speed",
                self.upload_speed,
                egui::Color32::from_rgb(76, 175, 80),
            );
            ui.add_space(10.0);
            self.traffic_card(
                ui,
                "download_speed",
                self.download_speed,
                egui::Color32::from_rgb(33, 150, 243),
            );
        });

        ui.add_space(20.0);
        // info!("Dashboard 2");

        // 显示连接数
        ui.horizontal(|ui| {
            ui.heading("connection count: ");
            ui.add_space(10.0);
            ui.label(format!("{}", self.connection_count));
        });

        ui.add_space(10.0);

        // 显示系统代理状态
        let is_system_proxy_enabled = false; // 实际应用中应该从系统获取
        ui.horizontal(|ui| {
            // info!("Dashboard 3");
            ui.heading("system proxy status");
            ui.add_space(10.0);
            let status_text = if is_system_proxy_enabled {
                "enabled"
            } else {
                "disabled"
            };
            let status_color = if is_system_proxy_enabled {
                egui::Color32::GREEN
            } else {
                egui::Color32::RED
            };
            ui.colored_label(status_color, status_text);

            if ui
                .button(if is_system_proxy_enabled {
                    "forbid"
                } else {
                    "start"
                })
                .clicked()
            {
                // 切换系统代理状态
                // 实际应用中应该调用系统API
            }
        });

        // info!("Dashboard 4");
        // 请求每秒更新一次UI
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
    }

    fn traffic_card(&self, ui: &mut egui::Ui, title: &str, value: f64, color: egui::Color32) {
        egui::Frame::new()
            .fill(ui.style().visuals.extreme_bg_color)
            .corner_radius(5.0)
            .shadow(egui::epaint::Shadow {
                offset: [1, 2],
                blur: 2,
                spread: 0,
                color: egui::Color32::from_black_alpha(40),
            })
            .show(ui, |ui| {
                ui.set_width(150.0);
                ui.set_height(100.0);
                ui.vertical_centered(|ui| {
                    ui.add_space(10.0);
                    ui.heading(title);
                    ui.add_space(5.0);
                    ui.label(
                        egui::RichText::new(format_speed(value))
                            .color(color)
                            .size(18.0),
                    );
                });
            });
    }

    fn update_traffic_data(&mut self) {
        // info!("update_traffic_data 0");
        if let Ok(core) = self.core.lock() {
            // info!("update_traffic_data 1");
            if let Ok(api_client) = core.get_api_client().lock() {
                // info!("update_traffic_data 2");
                if let Ok(traffic) = api_client.get_traffic() {
                    info!("update_traffic_data {:?}", traffic);
                    let now = std::time::Instant::now();
                    let elapsed = now.duration_since(self.last_update).as_secs_f64();

                    if elapsed > 0.0 && self.last_update.elapsed().as_secs() < 5 {
                        self.upload_speed = (traffic.up as f64 - self.last_upload as f64) / elapsed;
                        self.download_speed =
                            (traffic.down as f64 - self.last_download as f64) / elapsed;
                    }

                    self.last_upload = traffic.up;
                    self.last_download = traffic.down;
                    self.last_update = now;
                }

                // info!("update_traffic_data 4");
                // 获取连接数
                // 实际应用中应该从API获取
                self.connection_count = 0;
            }
        }
        // info!("update_traffic_data 4");
    }
}

fn format_speed(bytes_per_second: f64) -> String {
    if bytes_per_second < 1024.0 {
        format!("{:.1} B/s", bytes_per_second)
    } else if bytes_per_second < 1024.0 * 1024.0 {
        format!("{:.1} KB/s", bytes_per_second / 1024.0)
    } else if bytes_per_second < 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} MB/s", bytes_per_second / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB/s", bytes_per_second / (1024.0 * 1024.0 * 1024.0))
    }
}
