mod app;
mod clash;
mod ui;
mod utils;

use app::ClashApp;
use egui::IconData;
use log::info;

fn main() -> Result<(), eframe::Error> {
    // 初始化日志
    env_logger::init();
    info!("Starting Clash GUI...");

    // 设置应用程序选项
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 700.0])
            .with_icon(IconData::default())
            .with_min_inner_size([600.0, 400.0]),
        ..Default::default()
    };

    // 运行应用程序
    eframe::run_native(
        "Clash GUI",
        native_options,
        Box::new(|cc| Ok(Box::new(ClashApp::new(cc)))),
    )
}
