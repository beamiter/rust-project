//! System information display component

use std::sync::mpsc;

use crate::app::events::AppEvent;
use crate::constants::colors;
use crate::{app::state::AppState, constants::icons};
use egui::{Button, Label, Sense};

/// Controller information panel component
pub struct ControllerInfoPanel {}

impl ControllerInfoPanel {
    pub fn new() -> Self {
        Self {}
    }

    fn draw_battery_info(&self, ui: &mut egui::Ui, app_state: &mut AppState) {
        if let Some(snapshot) = app_state.system_monitor.get_snapshot() {
            // 获取电池电量百分比
            let battery_percent = snapshot.battery_percent;
            let is_charging = snapshot.is_charging;

            // 根据电量选择颜色
            let battery_color = match battery_percent {
                p if p > 50.0 => colors::BATTERY_HIGH,   // 高电量 - 绿色
                p if p > 20.0 => colors::BATTERY_MEDIUM, // 中电量 - 黄色
                _ => colors::BATTERY_LOW,                // 低电量 - 红色
            };

            // 显示电池图标和电量
            let battery_icon = if is_charging {
                "🔌" // 充电图标
            } else {
                match battery_percent {
                    p if p > 75.0 => "🔋", // 满电池
                    p if p > 50.0 => "🔋", // 高电量
                    p if p > 25.0 => "🪫", // 中电量
                    _ => "🪫",             // 低电量
                }
            };

            // 显示电池图标
            ui.label(egui::RichText::new(battery_icon).color(battery_color));

            // 显示电量百分比
            ui.label(egui::RichText::new(format!("{:.0}%", battery_percent)).color(battery_color));

            // 低电量警告
            if battery_percent < 0.2 * 100.0 && !is_charging {
                ui.label(egui::RichText::new("⚠️").color(colors::WARNING));
            }

            // 充电指示
            if is_charging {
                ui.label(egui::RichText::new("⚡").color(colors::CHARGING));
            }
        } else {
            // 无法获取电池信息时显示
            ui.label(egui::RichText::new("❓").color(colors::UNAVAILABLE));
        }
    }

    /// Draw volume control button
    fn draw_volume_button(&mut self, ui: &mut egui::Ui, app_state: &mut AppState) {
        let (volume_icon, tooltip) = if let Some(device) = app_state.get_master_audio_device() {
            let icon = if device.is_muted || device.volume == 0 {
                icons::VOLUME_MUTED
            } else if device.volume < 30 {
                icons::VOLUME_LOW
            } else if device.volume < 70 {
                icons::VOLUME_MEDIUM
            } else {
                icons::VOLUME_HIGH
            };

            let tooltip = format!(
                "{}：{}%{}",
                device.description,
                device.volume,
                if device.is_muted { " (已静音)" } else { "" }
            );

            (icon, tooltip)
        } else {
            (icons::VOLUME_MUTED, "无音频设备".to_string())
        };

        let label_response = ui.add(Button::new(volume_icon));
        if label_response.clicked() {
            app_state.ui_state.toggle_volume_window();
        }

        label_response.on_hover_text(tooltip);
    }

    /// Draw debug control button
    fn draw_debug_button(&mut self, ui: &mut egui::Ui, app_state: &mut AppState) {
        let (debug_icon, tooltip) = if app_state.ui_state.show_debug_window {
            ("󰱭", "关闭调试窗口") // 激活状态的图标和提示
        } else {
            ("🔍", "打开调试窗口") // 默认状态的图标和提示
        };

        let label_response = ui.add(Button::new(debug_icon).sense(Sense::click()));
        if label_response.clicked() {
            app_state.ui_state.toggle_debug_window();
        }

        // 添加详细的悬停提示信息
        let _detailed_tooltip = format!(
            "{}\n📊 性能: {:.1} FPS\n🧵 线程: {} 个活跃\n💾 内存: {:.1}%\n🖥️ CPU: {:.1}%",
            tooltip,
            app_state.performance_metrics.average_fps(),
            2, // 消息处理线程 + 定时更新线程
            app_state
                .system_monitor
                .get_snapshot()
                .map(|s| s.memory_usage_percent)
                .unwrap_or(0.0),
            app_state
                .system_monitor
                .get_snapshot()
                .map(|s| s.cpu_average)
                .unwrap_or(0.0)
        );

        // label_response.on_hover_text(detailed_tooltip);
    }

    /// Draw time display
    fn draw_time_display(
        &mut self,
        ui: &mut egui::Ui,
        app_state: &mut AppState,
        event_sender: &mpsc::Sender<AppEvent>,
    ) {
        let format_str = if app_state.ui_state.show_seconds {
            "%Y-%m-%d %H:%M:%S"
        } else {
            "%Y-%m-%d %H:%M"
        };

        let current_time = chrono::Local::now().format(format_str).to_string();

        if ui
            .selectable_label(
                true,
                egui::RichText::new(current_time)
                    .color(colors::GREEN)
                    .small(),
            )
            .clicked()
        {
            event_sender.send(AppEvent::TimeFormatToggle).ok();
        }
    }

    fn draw_screenshot_button(
        &mut self,
        ui: &mut egui::Ui,
        app_state: &mut AppState,
        event_sender: &mpsc::Sender<AppEvent>,
    ) {
        let label_response = ui.add(Button::new(format!(
            "{} {:.2}",
            icons::SCREENSHOT_ICON,
            app_state.ui_state.scale_factor
        )));

        if label_response.clicked() {
            event_sender.send(AppEvent::ScreenshotRequested).ok();
        }
    }

    fn draw_monitor_number(&mut self, ui: &mut egui::Ui, app_state: &mut AppState) {
        if let Some(ref message) = app_state.current_message {
            let monitor_num = (message.monitor_info.monitor_num as usize).min(1);
            ui.add(Label::new(
                egui::RichText::new(format!("{}", icons::MONITOR_NUMBERS[monitor_num])).strong(),
            ));
        }
    }

    /// Draw constoller information panel
    pub fn draw(
        &mut self,
        ui: &mut egui::Ui,
        app_state: &mut AppState,
        event_sender: &mpsc::Sender<AppEvent>,
    ) {
        // Battery info
        self.draw_battery_info(ui, app_state);

        // Volume button
        self.draw_volume_button(ui, app_state);

        // Debug button
        self.draw_debug_button(ui, app_state);

        // Time display
        self.draw_time_display(ui, app_state, event_sender);

        // Screenshot button
        self.draw_screenshot_button(ui, app_state, event_sender);

        // Monitor number
        self.draw_monitor_number(ui, app_state);
    }
}

impl Default for ControllerInfoPanel {
    fn default() -> Self {
        Self::new()
    }
}
