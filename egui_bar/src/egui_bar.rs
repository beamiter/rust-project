use bincode::deserialize;
use eframe::egui;
use egui::{Align, Color32, Layout, Vec2};
use shared_memory::{Shmem, ShmemConf};
use shared_structures::{SharedMessage, TagStatus};
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
    pub const RED: Color32 = Color32::from_rgb(255, 0, 0);
    pub const ORANGE: Color32 = Color32::from_rgb(255, 127, 0);
    pub const YELLOW: Color32 = Color32::from_rgb(255, 255, 0);
    pub const GREEN: Color32 = Color32::from_rgb(0, 255, 0);
    pub const BLUE: Color32 = Color32::from_rgb(0, 0, 255);
    pub const INDIGO: Color32 = Color32::from_rgb(75, 0, 130);
    pub const VIOLET: Color32 = Color32::from_rgb(148, 0, 211);
    pub const BROWN: Color32 = Color32::from_rgb(165, 42, 42);
    pub const GOLD: Color32 = Color32::from_rgb(255, 215, 0);
    pub const MAGENTA: Color32 = Color32::from_rgb(255, 0, 255);
    pub const CYAN: Color32 = Color32::from_rgb(0, 255, 255);
    pub const SILVER: Color32 = Color32::from_rgb(192, 192, 192);
    pub const OLIVE_GREEN: Color32 = Color32::from_rgb(128, 128, 0);
    pub const ROYALBLUE: Color32 = Color32::from_rgb(65, 105, 225);
    pub const TAG_COLORS: [Color32; 9] = [
        MyEguiApp::RED,
        MyEguiApp::ORANGE,
        MyEguiApp::YELLOW,
        MyEguiApp::GREEN,
        MyEguiApp::BLUE,
        MyEguiApp::INDIGO,
        MyEguiApp::VIOLET,
        MyEguiApp::BROWN,
        MyEguiApp::CYAN,
    ];
    pub const TAG_ICONS: [&str; 9] = [
        " 🍟 ", " 😃 ", " 🚀 ", " 🎉 ", " 🍕 ", " 🍖 ", " 🏍 ", " 🍔 ", " 🍘 ",
    ];

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
        // (TODO): Put this in single thread.
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
            let mut tag_status_vec: Vec<TagStatus> = Vec::new();
            let mut client_name = String::new();
            if let Some(ref message) = self.message {
                tag_status_vec = message.tag_status_vec.clone();
                client_name = message.client_name.clone();
            }
            ui.horizontal_centered(|ui| {
                for i in 0..MyEguiApp::TAG_ICONS.len() {
                    let tag_icon = MyEguiApp::TAG_ICONS.get(i).unwrap();
                    let tag_color = MyEguiApp::TAG_COLORS.get(i).unwrap();
                    let mut rich_text = egui::RichText::new(tag_icon.to_string())
                        .font(egui::FontId::monospace(MyEguiApp::FONT_SIZE));
                    if let Some(ref tag_status) = tag_status_vec.get(i) {
                        if tag_status.is_selected {
                            rich_text = rich_text.underline();
                        }
                        if tag_status.is_filled {
                            rich_text = rich_text.strong().italics();
                        }
                        if tag_status.is_occ {
                            rich_text = rich_text.color(tag_color.clone());
                            // rich_text = rich_text.background_color(tag_color.clone());
                        }
                    }
                    ui.label(rich_text);
                }
                ui.label(egui::RichText::new(" []= ").color(Color32::from_rgb(255, 0, 0)));

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let current_time = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                    ui.label(
                        egui::RichText::new(format!("{}", current_time))
                            .color(Color32::from_rgb(0, 255, 0)),
                    );
                    ui.label("current_time");

                    // ui.label(
                    //     egui::RichText::new(format!("{}", scale_factor))
                    //         .color(Color32::from_rgb(0, 255, 0)),
                    // );
                    // ui.label("scale_factor");

                    let average_duration = self.elapsed_duration.iter().sum::<Duration>()
                        / self.elapsed_duration.len() as u32;
                    let fps = 1.0 / average_duration.as_secs_f64();
                    ui.label(
                        egui::RichText::new(format!("{:.1}", fps))
                            .color(Color32::from_rgb(0, 255, 0)),
                    );
                    ui.label("FPS");
                    ui.horizontal(|ui| {
                        ui.with_layout(
                            egui::Layout::left_to_right(Align::Center).with_main_wrap(true),
                            |ui| {
                                // println!("client_name: {}", client_name);
                                ui.label(egui::RichText::new(format!("{}", client_name)).weak());
                            },
                        );
                    });
                });
            });
        });

        let screen_width = get_screen_width() / scale_factor;
        let width_offset = 6.0 / scale_factor;
        let desired_width = screen_width - width_offset;
        let hight_offset = 18.0 / scale_factor;
        let desired_height = MyEguiApp::FONT_SIZE + hight_offset;
        if desired_width != self.screen_rect_size.x {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2 {
                x: (desired_width),
                y: (desired_height),
            }));
        }

        ctx.request_repaint_after_secs(0.5);
    }
}
