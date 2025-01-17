use eframe::egui;
use std::fs::File;
use std::io::Read;

pub struct MyEguiApp {
    pipe: File,
    message: String,
}

impl MyEguiApp {
    pub fn new(pipe_path: String) -> Self {
        Self {
            pipe: File::open(pipe_path).unwrap(),
            message: String::new(),
        }
    }
}

impl eframe::App for MyEguiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 从管道读取消息
        let mut buffer = [0; 1024];
        if let Ok(size) = self.pipe.read(&mut buffer) {
            if size > 0 {
                self.message = String::from_utf8_lossy(&buffer[..size]).to_string();
            }
        }
        // 显示消息
        egui::CentralPanel::default().show(ctx, |ui| {
            println!("pipe info: {}", self.message);
            ui.label(&self.message);
        });
    }
}
