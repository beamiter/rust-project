//! egui_bar - A modern system status bar application

mod animation;
mod app;
mod config;
mod events;
mod ipc;
mod modules;
mod state;
mod theme;
mod windowing;

use config::CONFIG;
use log::info;
use std::env;
use xbar_core::initialize_logging;

/// Application entry point
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Parse command line arguments
    let args: Vec<String> = env::args().collect();
    let shared_path = args.get(1).cloned().unwrap_or_default();

    // Initialize logging
    if let Err(e) = initialize_logging("egui_bar", &shared_path) {
        eprintln!("Failed to initialize logging: {}", e);
        std::process::exit(1);
    }

    info!("Starting egui_bar V1.0");

    // Load config
    let cfg = CONFIG.load();

    // Transparent: env var overrides config
    let transparent = env::var("EGUI_BAR_TRANSPARENT")
        .ok()
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        || cfg.general.transparent;

    let height = cfg.general.height;

    // Capture the tokio runtime handle so modules (e.g. system tray) can
    // spawn async tasks even from eframe's non-tokio threads.
    let rt_handle = tokio::runtime::Handle::current();

    // Select and run the appropriate windowing backend
    let backend = windowing::select_backend(shared_path, transparent, height, rt_handle);
    backend.run()
}
