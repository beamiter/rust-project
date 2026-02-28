pub mod workspaces;
pub mod layout_selector;
pub mod clock;
pub mod cpu;
pub mod memory;
pub mod battery;
pub mod audio;
pub mod network;
pub mod bluetooth;
pub mod brightness;
pub mod media;
pub mod tray;

use crate::animation::AnimationState;
use crate::state::AppState;
use shared_structures::SharedRingBuffer;
use std::sync::Arc;

/// Trait for all bar modules
#[allow(dead_code)]
pub trait BarModule: Send {
    fn id(&self) -> &str;
    fn name(&self) -> &str;

    /// Update module state. Returns true if a repaint is needed.
    fn update(&mut self, state: &AppState) -> bool {
        let _ = state;
        false
    }

    /// Render the module's bar content
    fn render_bar(&mut self, ui: &mut egui::Ui, state: &mut AppState, anim: &mut AnimationState);

    /// Render popup/window if this module has one
    fn render_popup(&mut self, _ctx: &egui::Context, _state: &mut AppState) {}

    /// Whether this module has a popup
    fn has_popup(&self) -> bool {
        false
    }

    fn on_click(&mut self, _state: &mut AppState) {}
    fn on_secondary_click(&mut self, _state: &mut AppState) {}
    fn on_scroll(&mut self, _state: &mut AppState, _delta: f32) {}

    fn min_width(&self) -> Option<f32> {
        None
    }
}

/// Module registry that creates and manages bar modules
pub struct ModuleRegistry {
    pub left: Vec<Box<dyn BarModule>>,
    pub center: Vec<Box<dyn BarModule>>,
    pub right: Vec<Box<dyn BarModule>>,
}

impl ModuleRegistry {
    /// Create module registry from config
    pub fn from_config(
        shared_buffer: &Option<Arc<SharedRingBuffer>>,
        egui_ctx: &egui::Context,
        rt_handle: &tokio::runtime::Handle,
    ) -> Self {
        let cfg = crate::config::CONFIG.load();

        let left: Vec<Box<dyn BarModule>> = cfg
            .modules
            .left
            .iter()
            .filter_map(|name| create_module(name, shared_buffer, egui_ctx, rt_handle))
            .collect();

        let center: Vec<Box<dyn BarModule>> = cfg
            .modules
            .center
            .iter()
            .filter_map(|name| create_module(name, shared_buffer, egui_ctx, rt_handle))
            .collect();

        let right: Vec<Box<dyn BarModule>> = cfg
            .modules
            .right
            .iter()
            .filter_map(|name| create_module(name, shared_buffer, egui_ctx, rt_handle))
            .collect();

        Self { left, center, right }
    }
}

/// Factory function: create a module by name
fn create_module(
    name: &str,
    shared_buffer: &Option<Arc<SharedRingBuffer>>,
    egui_ctx: &egui::Context,
    rt_handle: &tokio::runtime::Handle,
) -> Option<Box<dyn BarModule>> {
    match name {
        "workspaces" => Some(Box::new(workspaces::WorkspacesModule::new(shared_buffer.clone()))),
        "layout" => Some(Box::new(layout_selector::LayoutSelectorModule::new(shared_buffer.clone()))),
        "clock" => Some(Box::new(clock::ClockModule::new())),
        "cpu" => Some(Box::new(cpu::CpuModule::new())),
        "memory" => Some(Box::new(memory::MemoryModule::new())),
        "battery" => Some(Box::new(battery::BatteryModule::new())),
        "audio" => Some(Box::new(audio::AudioModule::new())),
        "network" => Some(Box::new(network::NetworkModule::new())),
        "bluetooth" => Some(Box::new(bluetooth::BluetoothModule::new())),
        "brightness" => Some(Box::new(brightness::BrightnessModule::new())),
        "media" => Some(Box::new(media::MediaModule::new())),
        "tray" => Some(Box::new(tray::TrayModule::new(egui_ctx.clone(), rt_handle.clone()))),
        other => {
            log::warn!("Unknown module: {}", other);
            None
        }
    }
}
