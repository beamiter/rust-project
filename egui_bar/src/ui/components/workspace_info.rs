//! Workspace information display component

use log::info;
use shared_structures::{CommandType, SharedCommand};

use crate::app::state::AppState;
use crate::constants::{colors, icons};
use std::sync::mpsc;

/// Workspace panel component
pub struct WorkspacePanel;

impl WorkspacePanel {
    pub fn new() -> Self {
        Self
    }

    /// Draw workspace information
    pub fn draw(
        &self,
        ui: &mut egui::Ui,
        app_state: &AppState,
        command_sender: &mpsc::Sender<SharedCommand>,
    ) {
        let mut tag_status_vec = Vec::new();
        let mut layout_symbol = String::from(" ? ");

        if let Some(ref message) = app_state.current_message {
            tag_status_vec = message.monitor_info.tag_status_vec.clone();
            layout_symbol = message.monitor_info.ltsymbol.clone();
        }

        // Draw tag icons as buttons
        for (i, &tag_icon) in icons::TAG_ICONS.iter().enumerate() {
            let tag_color = colors::TAG_COLORS[i];
            let mut rich_text = egui::RichText::new(tag_icon).monospace();
            let tag_bit = 1 << i; // 计算标签位

            if let Some(tag_status) = tag_status_vec.get(i) {
                if tag_status.is_selected {
                    rich_text = rich_text.underline();
                }
                if tag_status.is_filled {
                    rich_text = rich_text.strong().italics();
                }
                if tag_status.is_occ {
                    rich_text = rich_text.color(tag_color);
                }
                if tag_status.is_urg {
                    rich_text = rich_text.background_color(colors::WHEAT);
                }
            }

            // 创建一个按钮而不是标签
            let button = ui.add(
                egui::Button::new(rich_text).small(), // 可选，使按钮更紧凑
                                                      // .frame(false), // 可选，使按钮看起来更像标签
            );

            // 处理点击事件 - 发送 ViewTag 命令
            if button.clicked() {
                info!("{} clicked", tag_bit);
                if let Some(ref message) = app_state.current_message {
                    let monitor_id = message.monitor_info.monitor_num;
                    let command = SharedCommand::view_tag(tag_bit, monitor_id);

                    // 发送命令到JWM
                    if let Err(e) = command_sender.send(command) {
                        log::error!("Failed to send ViewTag command: {}", e);
                    } else {
                        log::info!("Sent ViewTag command for tag {} in channel", i + 1);
                    }
                }
            }

            // 处理右键点击 - 发送 ToggleTag 命令
            if button.secondary_clicked() {
                info!("{} secondary_clicked", tag_bit);
                if let Some(ref message) = app_state.current_message {
                    let monitor_id = message.monitor_info.monitor_num;
                    let command = SharedCommand::toggle_tag(tag_bit, monitor_id);

                    // 发送命令到JWM
                    if let Err(e) = command_sender.send(command) {
                        log::error!("Failed to send ToggleTag command: {}", e);
                    } else {
                        log::info!("Sent ToggleTag command for tag {} in channel", i + 1);
                    }
                }
            }

            // 保留工具提示功能
            if let Some(tag_status) = tag_status_vec.get(i) {
                let mut tooltip = format!("标签 {}", i + 1);
                if tag_status.is_selected {
                    tooltip.push_str(" (当前)");
                }
                if tag_status.is_filled {
                    tooltip.push_str(" (有窗口)");
                }
                if tag_status.is_urg {
                    tooltip.push_str(" (紧急)");
                }
                button.on_hover_text(tooltip);
            }
        }

        // 布局符号也改为按钮
        let layout_button = ui.add(
            egui::Button::new(egui::RichText::new(layout_symbol).color(colors::ERROR)).small(), // .frame(false),
        );

        // 处理布局按钮点击
        if layout_button.clicked() {
            info!("layout_button clicked");
            if let Some(ref message) = app_state.current_message {
                let monitor_id = message.monitor_info.monitor_num;
                // 假设我们有一个切换布局的命令类型
                let command = SharedCommand::new(CommandType::SetLayout, 0, monitor_id); // 0表示循环到下一个布局

                if let Err(e) = command_sender.send(command) {
                    log::error!("Failed to send SetLayout command: {}", e);
                } else {
                    log::info!("Sent SetLayout command");
                }
            }
        }

        // 布局按钮的工具提示
        layout_button.on_hover_text("点击切换布局");
    }
}

impl Default for WorkspacePanel {
    fn default() -> Self {
        Self::new()
    }
}
