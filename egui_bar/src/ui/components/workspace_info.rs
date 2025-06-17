//! Workspace information display component

use egui::{Color32, Rect, Sense, Stroke, StrokeKind};
use egui_twemoji::EmojiLabel;
use log::info;
use shared_structures::{CommandType, SharedCommand};

use crate::app::state::AppState;
use crate::constants::{colors, icons};
use std::sync::mpsc;

/// Workspace panel component
pub struct WorkspacePanel {}

impl WorkspacePanel {
    pub fn new() -> Self {
        Self {}
    }

    /// Draw workspace information
    pub fn draw(
        &mut self,
        ui: &mut egui::Ui,
        app_state: &mut AppState,
        command_sender: &mpsc::Sender<SharedCommand>,
    ) {
        let mut tag_status_vec = Vec::new();
        let mut layout_symbol = String::from(" ? ");
        let spacing = 3.0;
        let bold_thickness = 2.5;
        let light_thickness = 1.0;

        if let Some(ref message) = app_state.current_message {
            tag_status_vec = message.monitor_info.tag_status_vec.clone();
            layout_symbol = message.monitor_info.ltsymbol.clone();
        }

        let style = ui.style();
        let window_margin = style.spacing.window_margin; // Margin

        let mut previous_rect: Option<Rect> = None;
        // Draw tag icons as buttons
        for (i, &tag_icon) in icons::TAG_ICONS.iter().enumerate() {
            ui.add_space(spacing);
            let tag_color = colors::TAG_COLORS[i];
            let tag_bit = 1 << i;

            // 构建基础文本样式
            let rich_text = egui::RichText::new(tag_icon).monospace();

            // 设置工具提示文本
            let mut tooltip = format!("标签 {}", i + 1);

            // 根据状态设置样式
            if let Some(tag_status) = tag_status_vec.get(i) {
                if tag_status.is_filled {
                    tooltip.push_str(" (有窗口)");
                }

                // is_selected: 当前标签标记
                if tag_status.is_selected {
                    tooltip.push_str(" (当前)");
                }

                // is_urg: 紧急状态标记
                if tag_status.is_urg {
                    tooltip.push_str(" (紧急)");
                }
            }

            // 创建可点击标签
            let label_response = EmojiLabel::new(rich_text).sense(Sense::click()).show(ui);

            // 绘制各种装饰效果
            let rect = label_response.rect;
            let new_rect = if let Some(previous_rect) = previous_rect {
                Rect::from_min_max(
                    egui::Pos2 {
                        x: (previous_rect.max.x + 2.0 * spacing),
                        y: (previous_rect.min.y),
                    },
                    rect.max,
                )
                .expand(1.0)
            } else {
                Rect::from_min_max(
                    egui::Pos2 {
                        x: (rect.min.x + spacing + window_margin.leftf()),
                        y: (rect.min.y),
                    },
                    rect.max,
                )
                .expand(1.0)
            };
            if let Some(tag_status) = tag_status_vec.get(i) {
                if tag_status.is_selected {
                    let underline_color = if tag_status.is_occ {
                        tag_color
                    } else {
                        Color32::WHITE
                    };
                    ui.painter().line_segment(
                        [new_rect.left_bottom(), new_rect.right_bottom()],
                        Stroke::new(bold_thickness, underline_color),
                    );
                } else if tag_status.is_occ {
                    ui.painter().line_segment(
                        [new_rect.left_bottom(), new_rect.right_bottom()],
                        Stroke::new(light_thickness, tag_color),
                    );
                }

                // is_urg: 绘制wheat色边框
                if tag_status.is_urg {
                    ui.painter().rect_stroke(
                        new_rect,
                        0.0,
                        Stroke::new(bold_thickness, colors::WHEAT),
                        StrokeKind::Inside,
                    );
                }
            }

            // 处理交互事件
            self.handle_tag_interactions(&label_response, tag_bit, i, app_state, command_sender);

            // 悬停效果和工具提示
            if label_response.hovered() {
                ui.painter().rect_stroke(
                    new_rect,
                    1.0,
                    Stroke::new(bold_thickness, Color32::KHAKI),
                    StrokeKind::Inside,
                );
                label_response.on_hover_text(tooltip);
            }
            previous_rect = Some(rect);
            ui.add_space(spacing);
        }

        self.render_layout_section(ui, app_state, command_sender, &layout_symbol);
    }
    // 提取交互处理逻辑到单独函数
    fn handle_tag_interactions(
        &self,
        label_response: &egui::Response,
        tag_bit: u32,
        tag_index: usize,
        app_state: &AppState,
        command_sender: &mpsc::Sender<SharedCommand>,
    ) {
        // 左键点击 - ViewTag 命令
        if label_response.clicked() {
            info!("{} clicked", tag_bit);
            self.send_tag_command(app_state, command_sender, tag_bit, tag_index, true);
        }

        // 右键点击 - ToggleTag 命令
        if label_response.secondary_clicked() {
            info!("{} secondary_clicked", tag_bit);
            self.send_tag_command(app_state, command_sender, tag_bit, tag_index, false);
        }
    }

    // 提取命令发送逻辑
    fn send_tag_command(
        &self,
        app_state: &AppState,
        command_sender: &mpsc::Sender<SharedCommand>,
        tag_bit: u32,
        tag_index: usize,
        is_view: bool,
    ) {
        if let Some(ref message) = app_state.current_message {
            let monitor_id = message.monitor_info.monitor_num;

            let command = if is_view {
                SharedCommand::view_tag(tag_bit, monitor_id)
            } else {
                SharedCommand::toggle_tag(tag_bit, monitor_id)
            };

            match command_sender.send(command) {
                Ok(_) => {
                    let action = if is_view { "ViewTag" } else { "ToggleTag" };
                    log::info!(
                        "Sent {} command for tag {} in channel",
                        action,
                        tag_index + 1
                    );
                }
                Err(e) => {
                    let action = if is_view { "ViewTag" } else { "ToggleTag" };
                    log::error!("Failed to send {} command: {}", action, e);
                }
            }
        }
    }

    fn render_layout_section(
        &self,
        ui: &mut egui::Ui,
        app_state: &mut AppState,
        command_sender: &mpsc::Sender<SharedCommand>,
        layout_symbol: &str,
    ) {
        ui.separator();
        // 主布局按钮
        let main_layout_button = ui.add(
            egui::Button::new(egui::RichText::new(layout_symbol).color(
                if app_state.layout_selector_open {
                    colors::SUCCESS
                } else {
                    colors::ERROR
                },
            ))
            .small(),
        );

        // 处理主布局按钮点击
        if main_layout_button.clicked() {
            info!("Layout button clicked, toggling selector");
            app_state.layout_selector_open = !app_state.layout_selector_open;
        }

        // 如果选择器是展开的，显示布局选项
        if app_state.layout_selector_open {
            ui.separator();

            // 水平显示所有布局选项
            for layout in &app_state.available_layouts {
                let is_current = layout.symbol == layout_symbol;

                let layout_option_button = ui.add(
                    egui::Button::new(egui::RichText::new(&layout.symbol).color(if is_current {
                        colors::SUCCESS
                    } else {
                        colors::ROYALBLUE
                    }))
                    .small()
                    .selected(is_current),
                );

                // 处理布局选项点击
                if layout_option_button.clicked() && !is_current {
                    info!("Layout option clicked: {} ({})", layout.name, layout.symbol);

                    if let Some(ref message) = app_state.current_message {
                        let monitor_id = message.monitor_info.monitor_num;
                        let command =
                            SharedCommand::new(CommandType::SetLayout, layout.index, monitor_id);

                        if let Err(e) = command_sender.send(command) {
                            log::error!("Failed to send SetLayout command: {}", e);
                        } else {
                            log::info!("Sent SetLayout command: layout_index={}", layout.index);
                        }
                    }

                    // 选择后关闭选择器
                    app_state.layout_selector_open = false;
                }

                // 添加工具提示
                let hover_text = format!("点击切换布局: {}", layout.name);
                layout_option_button.on_hover_text(hover_text);
            }
        }
    }
}

impl Default for WorkspacePanel {
    fn default() -> Self {
        Self::new()
    }
}
