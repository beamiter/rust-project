use eframe::egui;
use egui::{Align, Color32, Layout, Pos2};
use shared_structures::{SharedMessage, TagStatus};
use std::sync::mpsc;

pub struct MyEguiApp {
    message: Option<SharedMessage>,
    id: usize,
    receiver: mpsc::Receiver<SharedMessage>,
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
    pub const LYRIC: [usize; 14] = [0, 1, 2, 3, 4, 5, 6, 7, 6, 5, 4, 3, 2, 1];
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

    pub fn new(_cc: &eframe::CreationContext<'_>, receiver: mpsc::Receiver<SharedMessage>) -> Self {
        Self {
            message: None,
            id: 0,
            receiver,
        }
    }
}

#[allow(dead_code)]
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
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let _cpu_usage = frame.info().cpu_usage.unwrap_or(0.);
        self.id = self.id.wrapping_add(1).wrapping_rem(MyEguiApp::LYRIC.len());
        // println!("frame id: {}", self.id);
        while let Ok(message) = self.receiver.try_recv() {
            self.message = Some(message);
        }
        // println!("receive message: {:?}", self.message);
        let scale_factor = ctx.pixels_per_point();
        let screen_rect = ctx.screen_rect();
        // println!("screen_rect {}", screen_rect);
        let outer_rect = ctx.input(|i| i.viewport().outer_rect).unwrap();
        // println!("outer_rect {:?}", outer_rect);
        // let inner_rect = ctx.input(|i| i.viewport().inner_rect).unwrap();
        // println!("inner_rect {:?}", inner_rect);

        // print!("{:?}", viewport);
        egui::CentralPanel::default().show(ctx, |ui| {
            // self.viewpoint_size = ui.available_size();
            let mut tag_status_vec: Vec<TagStatus> = Vec::new();
            let mut client_name = String::new();
            // (TODO): support multi-monitor.
            if let Some(ref message) = self.message {
                tag_status_vec = message.monitor_info.tag_status_vec.clone();
                client_name = message.monitor_info.client_name.clone();
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
                        }
                        if tag_status.is_urg {
                            rich_text = rich_text.background_color(tag_color.clone());
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

                    ui.label(
                        egui::RichText::new(format!(
                            "{}",
                            "⬅".repeat(*MyEguiApp::LYRIC.get(self.id).unwrap_or(&0))
                        ))
                        .color(MyEguiApp::OLIVE_GREEN)
                        .font(egui::FontId::monospace(MyEguiApp::FONT_SIZE / 2.)),
                    );
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

        if let Some(message) = self.message.as_ref() {
            let monitor_width = message.monitor_info.monitor_width as f32 / scale_factor;
            let width_offset = 6.0 / scale_factor;
            let desired_width = monitor_width - width_offset;
            let hight_offset = 18.0 / scale_factor;
            let desired_height = MyEguiApp::FONT_SIZE + hight_offset;
            let desired_size = egui::Vec2::new(desired_width, desired_height);
            if desired_width != screen_rect.size().x {
                // println!(" desired_size: {}", desired_size);
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(desired_size));
            }
            let outer_rect_min = outer_rect.min;
            let desired_x = message.monitor_info.monitor_x as f32 + 2.;
            let desired_y = message.monitor_info.monitor_y as f32 + 1.;
            if desired_x != outer_rect_min.x - 1. && desired_y != outer_rect_min.y - 1. {
                let desired_outer_position = Pos2::new(desired_x as f32, desired_y as f32);
                // println!(" desired_outer_position: {}", desired_outer_position);
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(desired_outer_position));
            }
        }
    }
}
