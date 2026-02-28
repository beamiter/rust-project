use anyhow::Result;
use log::{error, info};

use crate::app::EguiBarApp;
use super::WindowingBackend;

/// Eframe/winit backend (X11 and basic Wayland via winit)
pub struct EframeBackend {
    shared_path: String,
    transparent: bool,
    height: f32,
    rt_handle: tokio::runtime::Handle,
}

impl EframeBackend {
    pub fn new(shared_path: String, transparent: bool, height: f32, rt_handle: tokio::runtime::Handle) -> Self {
        Self {
            shared_path,
            transparent,
            height,
            rt_handle,
        }
    }
}

impl WindowingBackend for EframeBackend {
    fn run(self: Box<Self>) -> Result<()> {
        let native_options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_position(egui::Pos2::new(0.0, 0.0))
                .with_inner_size([1080.0, self.height])
                .with_min_inner_size([480.0, self.height])
                .with_decorations(false)
                .with_resizable(true)
                .with_transparent(self.transparent),
            vsync: true,
            ..Default::default()
        };

        let shared_path = self.shared_path;
        let rt_handle = self.rt_handle;

        eframe::run_native(
            "egui_bar",
            native_options,
            Box::new(move |cc| match EguiBarApp::new(cc, shared_path, rt_handle) {
                Ok(app) => {
                    info!("Application created successfully");
                    Ok(Box::new(app))
                }
                Err(e) => {
                    error!("Failed to create application: {}", e);
                    std::process::exit(1);
                }
            }),
        )
        .map_err(|e| anyhow::anyhow!("eframe error: {}", e))
    }
}
