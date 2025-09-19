//! egui_bar - A modern system status bar application

mod egui_bar_app;
use egui_bar_app::EguiBarApp;
use log::{error, info};
use std::env;
use xbar_core::initialize_logging;

/// Application entry point
#[tokio::main]
async fn main() -> eframe::Result<()> {
    // Parse command line arguments
    let args: Vec<String> = env::args().collect();
    let shared_path = args.get(1).cloned().unwrap_or_default();

    // Initialize logging
    if let Err(e) = initialize_logging("egui_bar", &shared_path) {
        eprintln!("Failed to initialize logging: {}", e);
        std::process::exit(1);
    }

    info!("Starting egui_bar V1.0");
    // Configure eframe options
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_position(egui::Pos2::new(0.0, 0.0))
            .with_inner_size([1080.0, 40.])
            .with_min_inner_size([480.0, 40.])
            .with_decorations(false)
            .with_resizable(true)
            .with_transparent(false),
        vsync: true,
        ..Default::default()
    };

    // Run the application
    eframe::run_native(
        "egui_bar",
        native_options,
        Box::new(move |cc| match EguiBarApp::new(cc, shared_path) {
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
}
