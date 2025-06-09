//! Workspace information display component

use log::info;

use crate::app::events::AppEvent;
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
        _event_sender: &mpsc::Sender<AppEvent>,
    ) {
        let mut tag_status_vec = Vec::new();
        let mut layout_symbol = String::from(" ? ");

        if let Some(ref message) = app_state.current_message {
            tag_status_vec = message.monitor_info.tag_status_vec.clone();
            layout_symbol = message.monitor_info.ltsymbol.clone();
        }

        // Draw tag icons
        for (i, &tag_icon) in icons::TAG_ICONS.iter().enumerate() {
            let tag_color = colors::TAG_COLORS[i];
            let mut rich_text = egui::RichText::new(tag_icon).monospace();

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
            // info!("[draw] {:?}", rich_text);

            let response = ui.label(rich_text);

            // Add tooltip with tag information
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
                response.on_hover_text(tooltip);
            }
        }

        // Layout symbol
        ui.label(egui::RichText::new(layout_symbol).color(colors::ERROR));
    }
}

impl Default for WorkspacePanel {
    fn default() -> Self {
        Self::new()
    }
}
