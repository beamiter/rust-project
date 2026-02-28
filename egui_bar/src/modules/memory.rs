use crate::animation::AnimationState;
use crate::state::AppState;
use crate::theme::colors;

use super::BarModule;

pub struct MemoryModule;

impl MemoryModule {
    pub fn new() -> Self {
        Self
    }
}

impl BarModule for MemoryModule {
    fn id(&self) -> &str {
        "memory"
    }

    fn name(&self) -> &str {
        "Memory"
    }

    fn render_bar(&mut self, ui: &mut egui::Ui, state: &mut AppState, _anim: &mut AnimationState) {
        let (available_gb, used_gb) = state.get_memory_display_info();

        ui.label(
            egui::RichText::new(format!("{:.1}G", available_gb)).color(colors::MEMORY_AVAILABLE),
        );

        ui.label(egui::RichText::new(format!("{:.1}G", used_gb)).color(colors::MEMORY_USED));

        if let Some(snapshot) = state.system_monitor.get_snapshot() {
            if snapshot.memory_usage_percent > 0.8 * 100.0 {
                ui.label("⚠️");
            }
        }
        ui.separator();
    }
}
