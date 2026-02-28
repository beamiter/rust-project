use anyhow::Result;
use log::warn;

use super::WindowingBackend;

/// Wayland layer-shell backend
///
/// This backend creates a proper wlr-layer-shell surface for the bar,
/// ensuring it reserves exclusive zone and stays on top.
///
/// Current implementation: falls back to eframe with a warning,
/// as full layer-shell + egui rendering requires significant plumbing.
/// The infrastructure is here for incremental development.
pub struct LayerShellBackend {
    shared_path: String,
    transparent: bool,
    height: f32,
    rt_handle: tokio::runtime::Handle,
}

impl LayerShellBackend {
    pub fn new(shared_path: String, transparent: bool, height: f32, rt_handle: tokio::runtime::Handle) -> Self {
        Self {
            shared_path,
            transparent,
            height,
            rt_handle,
        }
    }
}

impl WindowingBackend for LayerShellBackend {
    fn run(self: Box<Self>) -> Result<()> {
        // Phase 4 POC: log that we detected Wayland, then delegate to eframe
        // which will use its own Wayland/winit path.
        //
        // Full layer-shell integration (zwlr_layer_surface_v1 + wgpu + egui)
        // is a large undertaking; this sets up the architecture so it can be
        // done incrementally without touching existing code.
        warn!(
            "Layer-shell backend selected but full implementation pending. \
             Falling back to eframe Wayland path."
        );

        let eframe_backend = super::eframe_backend::EframeBackend::new(
            self.shared_path,
            self.transparent,
            self.height,
            self.rt_handle,
        );
        Box::new(eframe_backend).run()
    }
}
