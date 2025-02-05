use eframe::egui;
use egui::{Align, Layout};
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
    pub const FONT_SIZE: f32 = 16.0;
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
            ui.horizontal_centered(|ui| {
                for cs in [
                    " 🍟 ", " 😃 ", " 🚀 ", " 🎉 ", " 🍕 ", " 🍖 ", " 🏍 ", " 🍔 ", " 🍘 ",
                ] {
                    ui.label(
                        egui::RichText::new(cs).font(egui::FontId::monospace(MyEguiApp::FONT_SIZE)),
                    );
                }
                ui.label(egui::RichText::new(" []= ").color(egui::Color32::from_rgb(255, 0, 0)));

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // Calculate the time difference between frames
                    let now = Instant::now();
                    let elapsed = now.duration_since(self.last_update);
                    self.last_update = now;
                    self.current_time =
                        chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                    ui.label(
                        egui::RichText::new(format!("{}", self.current_time))
                            .color(egui::Color32::from_rgb(0, 255, 0)),
                    );
                    ui.label("current_time");

                    // Store the frame durations
                    self.frame_durations.push(elapsed);
                    if self.frame_durations.len() > 2 {
                        self.frame_durations.remove(0); // Keep only the latest 100 frames
                    }

                    let scale_factor = ctx.pixels_per_point();
                    ui.label(
                        egui::RichText::new(format!("{}", scale_factor))
                            .color(egui::Color32::from_rgb(0, 255, 0)),
                    );
                    ui.label("scale_factor");

                    // Calculate the average frame duration and FPS
                    let avg_frame_duration: Duration =
                        self.frame_durations.iter().sum::<Duration>()
                            / self.frame_durations.len() as u32;
                    let fps = 1.0 / avg_frame_duration.as_secs_f64();
                    ui.label(
                        egui::RichText::new(format!("{:.2}", fps))
                            .color(egui::Color32::from_rgb(0, 255, 0)), // .font(egui::FontId::monospace(MyEguiApp::FONT_SIZE)),
                    );
                    ui.label("FPS");
                });
            });
        });
        ctx.request_repaint_after_secs(0.5);
    }
}
