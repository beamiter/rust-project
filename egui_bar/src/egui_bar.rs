use bincode::deserialize;
use eframe::egui;
use egui::{Align, Layout, Vec2};
use shared_memory::{Shmem, ShmemConf};
use shared_structures::SharedMessage;
use std::{
    collections::VecDeque,
    sync::mpsc,
    time::{Duration, Instant},
};

pub struct MyEguiApp {
    message: Option<SharedMessage>,
    shmem: Option<Shmem>,
    update_time: Instant,
    elapsed_duration: VecDeque<Duration>,
    screen_rect_size: Vec2,
    counter: usize,
    receiver: mpsc::Receiver<usize>,
}

impl MyEguiApp {
    pub const FONT_SIZE: f32 = 16.0;
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        receiver: mpsc::Receiver<usize>,
        shared_path: String,
    ) -> Self {
        Self {
            message: None,
            shmem: if shared_path.is_empty() {
                None
            } else {
                Some(ShmemConf::new().flink(shared_path).open().unwrap())
            },
            update_time: Instant::now(),
            elapsed_duration: VecDeque::with_capacity(2),
            screen_rect_size: Vec2::ZERO,
            counter: 0,
            receiver,
        }
    }

    fn read_message(&mut self) -> std::io::Result<SharedMessage> {
        if let Some(shmem) = self.shmem.as_mut() {
            let data = shmem.as_ptr();
            let serialized = unsafe { std::slice::from_raw_parts(data, shmem.len()) };
            let message: SharedMessage = deserialize(serialized).unwrap();
            // println!(
            //     "Process egui_bar: Data read from shared memory: {:?}, shmem len: {}",
            //     message,
            //     shmem.len()
            // );
            return Ok(message);
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "shmem is empty",
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
        if self.elapsed_duration.len() >= self.elapsed_duration.capacity() {
            self.elapsed_duration.pop_front();
        }
        self.elapsed_duration
            .push_back(Instant::now().duration_since(self.update_time));
        self.update_time = Instant::now();
        if let Ok(message) = self.read_message() {
            self.message = Some(message);
        } else {
            self.message = None;
        }
        while let Ok(count) = self.receiver.try_recv() {
            self.counter = count;
            // println!("receive counter: {}", self.counter);
        }
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
                    let current_time = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                    ui.label(
                        egui::RichText::new(format!("{}", current_time))
                            .color(egui::Color32::from_rgb(0, 255, 0)),
                    );
                    ui.label("current_time");

                    ui.label(
                        egui::RichText::new(format!("{}", scale_factor))
                            .color(egui::Color32::from_rgb(0, 255, 0)),
                    );
                    ui.label("scale_factor");

                    let average_duration = self.elapsed_duration.iter().sum::<Duration>()
                        / self.elapsed_duration.len() as u32;
                    let fps = 1.0 / average_duration.as_secs_f64();
                    ui.label(
                        egui::RichText::new(format!("{:.1}", fps))
                            .color(egui::Color32::from_rgb(0, 255, 0)),
                    );
                    ui.label("FPS");
                });
            });
        });

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
