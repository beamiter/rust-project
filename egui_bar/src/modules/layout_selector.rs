use log::info;
use shared_structures::SharedRingBuffer;
use std::sync::Arc;

use crate::animation::AnimationState;
use crate::ipc;
use crate::state::AppState;
use crate::theme::colors;

use super::BarModule;

pub struct LayoutSelectorModule {
    shared_buffer: Option<Arc<SharedRingBuffer>>,
}

impl LayoutSelectorModule {
    pub fn new(shared_buffer: Option<Arc<SharedRingBuffer>>) -> Self {
        Self { shared_buffer }
    }
}

impl BarModule for LayoutSelectorModule {
    fn id(&self) -> &str {
        "layout"
    }

    fn name(&self) -> &str {
        "Layout Selector"
    }

    fn render_bar(&mut self, ui: &mut egui::Ui, state: &mut AppState, _anim: &mut AnimationState) {
        let layout_symbol = state
            .current_message
            .as_ref()
            .map(|m| m.monitor_info.get_ltsymbol())
            .unwrap_or_default();

        ui.separator();

        let main_layout_button = ui.add(
            egui::Button::new(egui::RichText::new(&layout_symbol).color(
                if state.layout_selector_open {
                    colors::SUCCESS
                } else {
                    colors::ERROR
                },
            ))
            .small(),
        );

        if main_layout_button.clicked() {
            info!("Layout button clicked, toggling selector");
            state.layout_selector_open = !state.layout_selector_open;
        }

        if state.layout_selector_open {
            ui.separator();

            for layout in state.available_layouts.clone() {
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

                if layout_option_button.clicked() && !is_current {
                    info!("Layout option clicked: {} ({})", layout.name, layout.symbol);
                    ipc::send_layout_command(
                        &self.shared_buffer,
                        &state.current_message,
                        layout.index,
                    );
                    state.layout_selector_open = false;
                }

                let hover_text = format!("Switch layout to: {}", layout.name);
                layout_option_button.on_hover_text(hover_text);
            }
        }
    }
}
