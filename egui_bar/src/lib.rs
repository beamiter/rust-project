//! egui_bar - A modern system status bar
//!
//! This crate provides a customizable system status bar built with egui,
//! featuring audio control, system monitoring, and workspace information.

pub mod app_state;
pub mod egui_bar_app;
pub mod audio_manager;
pub mod constants;
pub mod system_monitor;
pub mod ui;
pub mod error;
pub mod metrics;

// Re-exports for convenience
pub use egui_bar_app::EguiBarApp;
pub use error::{AppError, Result};
pub use app_state::AppState;
