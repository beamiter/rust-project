use eframe::egui;
use egui::{Align, Color32, Layout};
use egui_plot::{Line, Plot, PlotPoints};
use shared_structures::{SharedMessage, TagStatus};
use std::{f64::consts::PI, process::Command, sync::mpsc};
use sysinfo::System;

#[allow(unused)]
pub struct MyEguiApp {
    message: Option<SharedMessage>,
    receiver: mpsc::Receiver<SharedMessage>,
    sys: System,
    point_index: usize,
    points: Vec<[f64; 2]>,
    point_speed: usize,
    toggle_time_style: bool,
    visible: bool,
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
    pub const WHEAT: Color32 = Color32::from_rgb(245, 222, 179);
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
            receiver,
            sys: System::new_all(),
            point_index: 0,
            points: {
                let step_num = 60;
                let step: f64 = PI / step_num as f64;
                (-step_num..=step_num)
                    .map(|x| {
                        let tmp_x = x as f64 * step;
                        [tmp_x, tmp_x.cos()]
                    })
                    .collect()
            },
            point_speed: 2,
            toggle_time_style: false,
            visible: true,
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
        let desired_height = MyEguiApp::FONT_SIZE + 18.0;
        let _cpu_usage = frame.info().cpu_usage.unwrap_or(0.);
        while let Ok(message) = self.receiver.try_recv() {
            self.message = Some(message);
        }
        // println!("receive message: {:?}", self.message);
        let scale_factor = ctx.pixels_per_point();
        let screen_rect = ctx.screen_rect();
        // println!("screen_rect {}", screen_rect);
        // let outer_rect = ctx.input(|i| i.viewport().outer_rect);
        // println!("outer_rect {:?}", outer_rect);
        // let inner_rect = ctx.input(|i| i.viewport().inner_rect).unwrap();
        // println!("inner_rect {:?}", inner_rect);

        // print!("{:?}", viewport);
        egui::CentralPanel::default().show(ctx, |ui| {
            // self.viewpoint_size = ui.available_size();
            let mut tag_status_vec: Vec<TagStatus> = Vec::new();
            if let Some(ref message) = self.message {
                tag_status_vec = message.monitor_info.tag_status_vec.clone();
                let _client_name = message.monitor_info.client_name.clone();
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
                            rich_text = rich_text.background_color(MyEguiApp::WHEAT);
                        }
                    }
                    ui.label(rich_text);
                }
                ui.label(egui::RichText::new(" []= ").color(Color32::from_rgb(255, 0, 0)));
                let num_emoji_vec = vec!["⓪", "①"];
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let current_time = if self.toggle_time_style {
                        chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
                    } else {
                        chrono::Local::now().format("%Y-%m-%d %H:%M").to_string()
                    };
                    if ui
                        .selectable_label(
                            true,
                            egui::RichText::new(format!("{}", current_time))
                                .color(Color32::from_rgb(0, 255, 0)),
                        )
                        .clicked()
                    {
                        self.toggle_time_style = !self.toggle_time_style;
                    }
                    if ui.small_button(format!("ⓢ {:.2}", scale_factor)).clicked() {
                        let _ = Command::new("flameshot").arg("gui").spawn();
                    }
                    ui.label(
                        egui::RichText::new(format!(
                            "[{}]",
                            num_emoji_vec[self
                                .message
                                .clone()
                                .unwrap_or_default()
                                .monitor_info
                                .monitor_num as usize],
                        ))
                        .strong(),
                    );

                    self.sys.refresh_memory();
                    let unavailable =
                        (self.sys.total_memory() - self.sys.available_memory()) as f64 / 1e9;
                    ui.label(
                        egui::RichText::new(format!("{:.1}", unavailable)).color(MyEguiApp::SILVER),
                    );
                    let available = self.sys.available_memory() as f64 / 1e9;
                    ui.label(
                        egui::RichText::new(format!("{:.1}", available)).color(MyEguiApp::CYAN),
                    );
                    let reset_view = ui.small_button("R").clicked();

                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        let available_width = ui.available_width();
                        let plot_height = ui.available_height().max(desired_height);
                        let plot_width = (20.0 * plot_height).min(available_width * 0.5);
                        ui.add_space(available_width - plot_width - 2.);
                        let mut plot = Plot::new("live plot")
                            .x_axis_formatter(|_, _| String::new())
                            .y_axis_formatter(|_, _| String::new())
                            .width(plot_width)
                            .height(plot_height);
                        if reset_view {
                            self.point_index = 0;
                            plot = plot.reset();
                        }
                        let mut vis_points: Vec<[f64; 2]> = vec![];
                        for i in 0..self.points.len() {
                            let index = self
                                .point_index
                                .wrapping_add(i)
                                .wrapping_rem(self.points.len());
                            let x = self.points[i][0];
                            let y = self.points[index][1];
                            vis_points.push([x, y]);
                        }
                        self.point_index = self
                            .point_index
                            .wrapping_add(self.point_speed)
                            .wrapping_rem(self.points.len());
                        let line = Line::new(PlotPoints::from(vis_points)).name("cosine");
                        plot.show(ui, |plot_ui| {
                            plot_ui.line(line);
                        });
                    });

                    if let Some(message) = self.message.as_ref() {
                        self.visible = message.monitor_info.showbar0;
                        let monitor_width = message.monitor_info.monitor_width as f32;
                        let desired_width = monitor_width;
                        let desired_size =
                            egui::Vec2::new(desired_width / scale_factor, desired_height);
                        // No need to care about height
                        if desired_size.x != screen_rect.size().x {
                            // let size_log_info = format!(
                            //     "desired_size: {}, screen_rect: {};",
                            //     desired_size,
                            //     screen_rect.size()
                            // );
                            // ui.label(&size_log_info);
                            // println!("{}", size_log_info);
                            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                                egui::Pos2::ZERO,
                            ));
                            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(desired_size));
                        }
                    }
                });
            });
        });
    }
}
