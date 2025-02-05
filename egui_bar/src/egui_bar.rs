use eframe::egui;
use std::fs::File;
use std::io::Read;

pub struct MyEguiApp {
    message: String,
    pipe: std::io::Result<File>,
}

impl MyEguiApp {
    pub fn new(pipe_path: String) -> Self {
        Self {
            message: String::new(),
            pipe: File::open(pipe_path),
        }
    }
}

impl eframe::App for MyEguiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        match self.pipe {
            Ok(ref mut pipe) => {
                let mut buffer = [0; 1024];
                if let Ok(size) = pipe.read(&mut buffer) {
                    if size > 0 {
                        self.message = String::from_utf8_lossy(&buffer[..size]).to_string();
                    } else {
                        self.message = "empty pipe".to_string();
                    }
                }
            }
            Err(_) => {
                self.message = "fail to open pipe: 🍔🍕🍖🍗🍘🍟".to_string();
            }
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label("Here are some emojis:🌍 😃 🚀 🎉 🍕 🐱 🏍");
            ui.label(&self.message);
            ui.label(
                egui::RichText::new("Hello, world!")
                    .color(egui::Color32::from_rgb(0, 255, 0))
                    .size(24.0),
            );
            ui.label(egui::RichText::new("This is bold text").strong());
            ui.label(egui::RichText::new("This is italic text").italics());
            ui.label(
                egui::RichText::new("This text has a custom font 🌍 😃 🚀 🎉 🍕 🐱 🏍")
                    .font(egui::FontId::monospace(20.0)),
            );
            ui.label(
                egui::RichText::new("Red text with underline")
                    .color(egui::Color32::RED)
                    .underline(),
            );
            // 显示整个窗口的宽度
            let screen_rect = ctx.screen_rect();
            let width = screen_rect.width();
            let height = screen_rect.height();
            ui.label(format!(
                "Window logical width: {}, height: {}",
                width, height
            ));

            // 显示可用区域的宽度
            let available_size = ui.available_size();
            let width = available_size.x;
            let height = available_size.y;
            ui.label(format!(
                "UI available logical width: {}, height: {}",
                width, height
            ));
            let scale_factor = ctx.pixels_per_point();
            ui.label(format!("scale_factor: {}", scale_factor));
        });
    }
}
