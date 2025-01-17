mod egui_bar;
pub use egui_bar::MyEguiApp;
use std::env;

fn main() -> eframe::Result {
    let args: Vec<String> = env::args().collect();
    let pipe_path = &args[1];
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "My egui App",
        options,
        Box::new(|_cc| Ok(Box::new(MyEguiApp::new(pipe_path.to_string())))),
    )
}
