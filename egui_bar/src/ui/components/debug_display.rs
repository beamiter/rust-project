//! Volume control window component

use crate::app::events::AppEvent;
use crate::app::state::AppState;
use crate::constants::colors;
use egui::{Align, Layout};
use egui_twemoji::EmojiLabel;
use log::error;
use std::sync::mpsc;
use std::time::Instant;

/// Volume control window component
#[allow(dead_code)]
pub struct DebugDisplayWindow {
    last_render_time: Instant,
}

impl DebugDisplayWindow {
    pub fn new() -> Self {
        Self {
            last_render_time: Instant::now(),
        }
    }

    /// Draw volume control window, returns true if window was closed
    pub fn draw(
        &mut self,
        ctx: &egui::Context,
        app_state: &mut AppState,
        event_sender: &mpsc::Sender<AppEvent>,
    ) {
        if !app_state.ui_state.show_debug_window {
            return;
        }

        let mut window_open = true;

        egui::Window::new("🐛 调试信息")
            .collapsible(false)
            .resizable(true)
            .default_width(400.0)
            .default_height(300.0)
            .open(&mut window_open)
            .show(ctx, |ui| {
                EmojiLabel::new("📊 性能指标").show(ui);
                ui.horizontal(|ui| {
                    ui.label("FPS:");
                    ui.label(
                        egui::RichText::new(format!(
                            "{:.1}",
                            app_state.performance_metrics.average_fps()
                        ))
                        .color(colors::GREEN),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("帧时间:");
                    ui.label(format!(
                        "{:.2}ms",
                        app_state.performance_metrics.average_frame_time_ms()
                    ));
                });
                ui.horizontal(|ui| {
                    ui.label("渲染时间:");
                    ui.label(format!(
                        "{:.2}ms",
                        app_state.performance_metrics.average_render_time_ms()
                    ));
                });

                ui.separator();

                EmojiLabel::new("💻 系统状态").show(ui);
                if let Some(snapshot) = app_state.system_monitor.get_snapshot() {
                    ui.horizontal(|ui| {
                        ui.label("CPU:");
                        let cpu_color = if snapshot.cpu_average > 80.0 {
                            colors::ERROR
                        } else if snapshot.cpu_average > 60.0 {
                            colors::WARNING
                        } else {
                            colors::SUCCESS
                        };
                        ui.label(
                            egui::RichText::new(format!("{:.1}%", snapshot.cpu_average))
                                .color(cpu_color),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("内存:");
                        let mem_color = if snapshot.memory_usage_percent > 80.0 {
                            colors::ERROR
                        } else if snapshot.memory_usage_percent > 60.0 {
                            colors::WARNING
                        } else {
                            colors::SUCCESS
                        };
                        ui.label(
                            egui::RichText::new(format!("{:.1}%", snapshot.memory_usage_percent))
                                .color(mem_color),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("运行时间:");
                        ui.label(app_state.system_monitor.get_uptime_string());
                    });
                }

                ui.separator();

                EmojiLabel::new("🔊 音频系统").show(ui);
                let stats = app_state.audio_manager.get_stats();
                ui.horizontal(|ui| {
                    ui.label("设备数量:");
                    ui.label(format!("{}", stats.total_devices));
                });
                ui.horizontal(|ui| {
                    ui.label("可控音量:");
                    ui.label(format!("{}", stats.devices_with_volume));
                });
                ui.horizontal(|ui| {
                    ui.label("已静音:");
                    ui.label(format!("{}", stats.muted_devices));
                });

                ui.separator();

                // 操作按钮
                ui.horizontal(|ui| {
                    if ui.small_button("💾 保存配置").clicked() {
                        event_sender.send(AppEvent::SaveConfig).ok();
                    }

                    if ui.small_button("🔄 刷新音频").clicked() {
                        if let Err(e) = app_state.audio_manager.refresh_devices() {
                            error!("Failed to refresh audio devices: {}", e);
                        }
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("❌ 关闭").clicked() {
                            app_state.ui_state.toggle_debug_window();
                        }
                    });
                });
            });

        if !window_open || ctx.input(|i| i.viewport().close_requested()) {
            app_state.ui_state.toggle_debug_window();
        }
    }
}

impl Default for DebugDisplayWindow {
    fn default() -> Self {
        Self::new()
    }
}
