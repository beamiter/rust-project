use bincode::deserialize;
use eframe::egui;
use egui::{Align, Layout, Vec2};
use shared_structures::SharedMessage;
use std::fs::File;
use std::io::Read;
use std::time::{Duration, Instant};

pub struct MyEguiApp {
    message: Option<SharedMessage>,
    pipe: Option<File>,
    last_update: Instant,
    frame_durations: Vec<Duration>,
    current_time: String,
    screen_rect_size: Vec2,
}

impl MyEguiApp {
    pub const FONT_SIZE: f32 = 16.0;
    pub fn new(pipe_path: String) -> Self {
        Self {
            message: None,
            pipe: if pipe_path.is_empty() {
                None
            } else {
                Some(File::open(pipe_path).unwrap())
            },
            last_update: Instant::now(),
            frame_durations: Vec::with_capacity(100),
            current_time: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            screen_rect_size: Vec2::ZERO,
        }
    }

    fn read_message(&mut self) -> std::io::Result<SharedMessage> {
        if let Some(file) = self.pipe.as_mut() {
            let mut len_bytes = [0u8; 4];
            file.read_exact(&mut len_bytes)?;
            let len = u32::from_le_bytes(len_bytes) as usize;
            let mut buffer = vec![0u8; len];
            file.read_exact(&mut buffer)?;
            let message: SharedMessage = deserialize(&buffer).expect("Deserialization failed");
            return Ok(message);
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "pipe is empty",
        ))
    }
}

fn get_screen_width() -> f32 {
    #[cfg(target_os = "linux")]
    {
        use x11rb::connection::Connection;
        let (conn, screen_num) = x11rb::connect(None).unwrap();
        let screen = &conn.setup().roots[screen_num];
        screen.width_in_pixels as f32
    }
}

impl eframe::App for MyEguiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.message = Some(self.read_message().unwrap());
        // println!("{:?}", self.message);
        let scale_factor = ctx.pixels_per_point();
        self.screen_rect_size = ctx.screen_rect().size();
        egui::CentralPanel::default().show(ctx, |ui| {
            // self.viewpoint_size = ui.available_size();
            ui.horizontal_centered(|ui| {
                for cs in [
                    " 🍟 ", " 😃 ", " 🚀 ", " 🎉 ", " 🍕 ", " 🍖 ", " 🏍 ", " 🍔 ", " 🍘 ",
                ] {
                    ui.label(
                        egui::RichText::new(cs).font(egui::FontId::monospace(MyEguiApp::FONT_SIZE)),
                    );
                }
                ui.label(egui::RichText::new(" []= ").color(egui::Color32::from_rgb(255, 0, 0)));

                ui.label(format!("message: {:?}", self.message));

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
                    if self.frame_durations.len() > 1 {
                        self.frame_durations.remove(0); // Keep only the latest 100 frames
                    }

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
                            .color(egui::Color32::from_rgb(0, 255, 0)),
                    );
                    ui.label("FPS");
                });
            });
        });
        ctx.request_repaint_after_secs(0.25);

        let screen_width = get_screen_width() / scale_factor;
        let width_offset = 6.0;
        let desired_width = screen_width - width_offset;
        let hight_offset = 16.0;
        let desired_height = MyEguiApp::FONT_SIZE + hight_offset;
        if desired_width != self.screen_rect_size.x {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2 {
                x: (desired_width),
                y: (desired_height),
            }));
        }
    }
}
