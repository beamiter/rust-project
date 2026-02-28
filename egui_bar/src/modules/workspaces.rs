use egui::{Button, Color32, Stroke, StrokeKind, Vec2};
use log::info;
use shared_structures::SharedRingBuffer;
use std::sync::Arc;

use crate::animation::AnimationState;
use crate::config::EasingName;
use crate::ipc;
use crate::state::AppState;
use crate::theme::{colors, icons, with_alpha};

use super::BarModule;

pub struct WorkspacesModule {
    shared_buffer: Option<Arc<SharedRingBuffer>>,
}

impl WorkspacesModule {
    pub fn new(shared_buffer: Option<Arc<SharedRingBuffer>>) -> Self {
        Self { shared_buffer }
    }
}

impl BarModule for WorkspacesModule {
    fn id(&self) -> &str {
        "workspaces"
    }

    fn name(&self) -> &str {
        "Workspaces"
    }

    fn render_bar(&mut self, ui: &mut egui::Ui, state: &mut AppState, anim: &mut AnimationState) {
        let bold_thickness = 2.5;
        let light_thickness = 1.5;
        let monitor_info = state
            .current_message
            .as_ref()
            .map(|m| m.monitor_info)
            .unwrap_or_default();

        let cfg = crate::config::CONFIG.load();
        let hover_ms = cfg.animation.hover_duration_ms;
        let easing = cfg.animation.easing;

        for (index, &tag_icon) in icons::TAG_ICONS.iter().enumerate() {
            let tag_color = colors::TAG_COLORS[index];
            let tag_bit = 1 << index;

            let rich_text = egui::RichText::new(tag_icon).monospace();

            let mut is_urg = false;
            let mut is_filled = false;
            let mut is_selected = false;
            let mut tooltip = format!("Tag {}", index + 1);
            let mut button_bg_color = Color32::TRANSPARENT;

            if let Some(tag_status) = monitor_info.tag_status_vec.get(index) {
                if tag_status.is_urg {
                    tooltip.push_str(" (urgent)");
                    is_urg = true;
                    button_bg_color = with_alpha(colors::RED, 90);
                } else if tag_status.is_filled {
                    is_filled = true;
                    tooltip.push_str(" (has windows)");
                    button_bg_color = with_alpha(tag_color, 55);
                } else if tag_status.is_selected {
                    tooltip.push_str(" (current)");
                    is_selected = true;
                    button_bg_color = with_alpha(tag_color, 85);
                } else if tag_status.is_occ {
                    button_bg_color = with_alpha(tag_color, 40);
                }
            }

            // Animate background color
            let anim_id = format!("ws_bg_{}", index);
            let bg = anim.animate_color(&anim_id, button_bg_color, hover_ms, easing);

            let button = Button::new(rich_text)
                .min_size(Vec2::new(34.0, 26.0))
                .fill(bg);

            let label_response = ui.add(button);
            let rect = label_response.rect;
            state.ui_state.button_height = rect.height();

            // Draw border decorations
            if is_urg {
                ui.painter().rect_stroke(
                    rect,
                    1.0,
                    Stroke::new(bold_thickness, colors::VIOLET),
                    StrokeKind::Inside,
                );
            } else if is_filled {
                ui.painter().rect_stroke(
                    rect,
                    1.0,
                    Stroke::new(bold_thickness, tag_color),
                    StrokeKind::Inside,
                );
            } else if is_selected {
                ui.painter().rect_stroke(
                    rect,
                    1.0,
                    Stroke::new(light_thickness, tag_color),
                    StrokeKind::Inside,
                );
            }

            // Handle interactions
            if label_response.clicked() {
                info!("{} clicked", tag_bit);
                ipc::send_tag_command(
                    &self.shared_buffer,
                    &state.current_message,
                    tag_bit,
                    true,
                );
            }

            if label_response.secondary_clicked() {
                info!("{} secondary_clicked", tag_bit);
                ipc::send_tag_command(
                    &self.shared_buffer,
                    &state.current_message,
                    tag_bit,
                    false,
                );
            }

            // Hover effects
            let hover_alpha = if label_response.hovered() { 1.0_f32 } else { 0.0_f32 };
            let hover_anim = anim.animate(
                &format!("ws_hover_{}", index),
                hover_alpha,
                hover_ms,
                EasingName::EaseOutQuad,
            );

            if hover_anim > 0.01 {
                let expand = hover_anim * 1.0;
                let alpha = (hover_anim * 255.0) as u8;
                ui.painter().rect_stroke(
                    rect.expand(expand),
                    1.0,
                    Stroke::new(bold_thickness, with_alpha(tag_color, alpha)),
                    StrokeKind::Inside,
                );
            }

            label_response.on_hover_text(tooltip);
        }
    }
}
