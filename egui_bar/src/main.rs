//! egui_bar - A modern system status bar application

use chrono::Local;
use egui_bar::{AppError, EguiBarApp};
use flexi_logger::{Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming};
use log::{error, info};
use std::env;
use std::path::Path;

/// Application entry point
#[tokio::main]
async fn main() -> eframe::Result<()> {
    // Parse command line arguments
    let args: Vec<String> = env::args().collect();
    let shared_path = args.get(1).cloned().unwrap_or_default();

    // Initialize logging
    if let Err(e) = initialize_logging(&shared_path) {
        eprintln!("Failed to initialize logging: {}", e);
        std::process::exit(1);
    }

    info!("Starting egui_bar V1.0");
    // Configure eframe options
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_position(egui::Pos2::new(0.0, 0.0))
            .with_inner_size([1080.0, 40.])
            .with_min_inner_size([480.0, 20.])
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
                // don't do this every frame - only when the app is created!
                egui_extras::install_image_loaders(&cc.egui_ctx);
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

/// Initialize logging system
fn initialize_logging(shared_path: &str) -> Result<(), AppError> {
    let now = Local::now();
    let timestamp = now.format("%Y-%m-%d_%H_%M_%S").to_string();

    let file_name = if shared_path.is_empty() {
        "egui_bar".to_string()
    } else {
        Path::new(shared_path)
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| format!("egui_bar_{}", name))
            .unwrap_or_else(|| "egui_bar".to_string())
    };

    let log_filename = format!("{}_{}", file_name, timestamp);

    Logger::try_with_str("info")
        .map_err(|e| AppError::config(format!("Failed to create logger: {}", e)))?
        .format(flexi_logger::colored_opt_format)
        .log_to_file(
            FileSpec::default()
                .directory("/tmp")
                .basename(log_filename)
                .suffix("log"),
        )
        .duplicate_to_stdout(Duplicate::Debug)
        .rotate(
            Criterion::Size(10_000_000), // 10MB
            Naming::Numbers,
            Cleanup::KeepLogFiles(5),
        )
        .start()
        .map_err(|e| AppError::config(format!("Failed to start logger: {}", e)))?;

    Ok(())
}
