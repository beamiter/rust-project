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
                self.message = "fail to open pipe".to_string();
            }
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label(&self.message);
        });
    }
}
