//! egui_bar - A modern system status bar
//!
//! This crate provides a customizable system status bar built with egui,
//! featuring audio control, system monitoring, and workspace information.

pub mod app;
pub mod audio;
pub mod constants;
pub mod system;
pub mod ui;
pub mod utils;

// Re-exports for convenience
pub use app::EguiBarApp;
pub use utils::error::{AppError, Result};
