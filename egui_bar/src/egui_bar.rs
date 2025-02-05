use eframe::egui;
use std::fs::File;
use std::io::Read;
use std::time::{Duration, Instant};

pub struct MyEguiApp {
    message: String,
    pipe: std::io::Result<File>,
    last_update: Instant,
    frame_durations: Vec<Duration>,
    current_time: String,
}

impl MyEguiApp {
    pub fn new(pipe_path: String) -> Self {
        Self {
            message: String::new(),
            pipe: File::open(pipe_path),
            last_update: Instant::now(),
            frame_durations: Vec::with_capacity(100),
            current_time: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
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
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("🍔 🍕 🍘 🍟 😃 🚀 🎉 🍕 🐱 🏍")
                        .font(egui::FontId::monospace(12.0)),
                );
                ui.label(
                    egui::RichText::new("Hello,")
                        .color(egui::Color32::from_rgb(0, 255, 0))
                        .size(12.0),
                );
                ui.label(
                    egui::RichText::new(" world!")
                        .color(egui::Color32::RED)
                        .size(12.0)
                        .underline()
                        .strong()
                        .italics(),
                );
                ui.label(egui::RichText::new(" []= ").font(egui::FontId::monospace(12.0)));
                let scale_factor = ctx.pixels_per_point();
                ui.label(format!("scale_factor: {}", scale_factor));

                // Calculate the time difference between frames
                let now = Instant::now();
                let elapsed = now.duration_since(self.last_update);
                self.last_update = now;
                self.current_time = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                ui.label(format!("Current time: {}", self.current_time));

                // Store the frame durations
                self.frame_durations.push(elapsed);
                if self.frame_durations.len() > 5 {
                    self.frame_durations.remove(0); // Keep only the latest 100 frames
                }

                // Calculate the average frame duration and FPS
                let avg_frame_duration: Duration = self.frame_durations.iter().sum::<Duration>()
                    / self.frame_durations.len() as u32;
                let fps = 1.0 / avg_frame_duration.as_secs_f64();
                ui.label(format!("FPS: {:.2}", fps));
            });
        });
        std::thread::sleep(Duration::from_millis(500));
        ctx.request_repaint();
    }
}
