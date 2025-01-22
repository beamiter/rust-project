mod egui_bar;
pub use egui_bar::MyEguiApp;
use std::env;

fn main() -> eframe::Result {
    let args: Vec<String> = env::args().collect();
    let pipe_path = &args[1];
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 50.0]) // Initial height
            .with_min_inner_size([1000.0, 40.0]) // Minimum size
            .with_decorations(false), // Hide title bar and decorations
            // .with_always_on_top(), // Keep window always on top
        ..Default::default()
    };
    eframe::run_native(
        "My egui App",
        native_options,
        Box::new(|_cc| Ok(Box::new(MyEguiApp::new(pipe_path.to_string())))),
    )
}
