use eframe::egui;
use egui::{Align, Color32, Layout};
use egui_plot::{Bar, BarChart, Line, Plot, PlotPoints};
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
    data: Vec<f64>,
}

impl MyEguiApp {
    pub const FONT_SIZE: f32 = 16.0;
    pub const DESIRED_HEIGHT: f32 = MyEguiApp::FONT_SIZE + 18.0;
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
            data: Vec::new(),
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

impl MyEguiApp {
    fn draw_smooth_gradient_line(&mut self, ui: &mut egui::Ui) {
        let reset_view = ui.small_button("R").clicked();

        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            let available_width = ui.available_width();
            let plot_height = ui.available_height().max(MyEguiApp::DESIRED_HEIGHT);
            let plot_width = (10.0 * plot_height).min(available_width * 0.5);
            ui.add_space(available_width - plot_width - 2.);

            let mut plot = Plot::new("GradientLineChart")
                .include_y(0.)
                .include_y(1.)
                .allow_zoom(true)
                .x_axis_formatter(|_, _| String::new())
                .y_axis_formatter(|_, _| String::new())
                .width(plot_width)
                .height(plot_height);

            if reset_view {
                plot = plot.reset();
            }

            plot.show(ui, |plot_ui| {
                if self.data.len() < 2 {
                    return; // 至少需要两个点
                }

                // 定义渐变色映射函数
                let color_at = |y: f64| -> egui::Color32 {
                    let y = y.clamp(0.0, 1.0) as f32;

                    // 使用更复杂的渐变色映射
                    if y < 0.2 {
                        // 蓝色到青色
                        let t = y / 0.2;
                        let r = 0;
                        let g = (t * 255.0) as u8;
                        let b = 255;
                        egui::Color32::from_rgb(r, g, b)
                    } else if y < 0.4 {
                        // 青色到绿色
                        let t = (y - 0.2) / 0.2;
                        let r = 0;
                        let g = 255;
                        let b = ((1.0 - t) * 255.0) as u8;
                        egui::Color32::from_rgb(r, g, b)
                    } else if y < 0.6 {
                        // 绿色到黄色
                        let t = (y - 0.4) / 0.2;
                        let r = (t * 255.0) as u8;
                        let g = 255;
                        let b = 0;
                        egui::Color32::from_rgb(r, g, b)
                    } else if y < 0.8 {
                        // 黄色到橙色
                        let t = (y - 0.6) / 0.2;
                        let r = 255;
                        let g = (255.0 * (1.0 - t * 0.5)) as u8;
                        let b = 0;
                        egui::Color32::from_rgb(r, g, b)
                    } else {
                        // 橙色到红色
                        let t = (y - 0.8) / 0.2;
                        let r = 255;
                        let g = (128.0 * (1.0 - t)) as u8;
                        let b = 0;
                        egui::Color32::from_rgb(r, g, b)
                    }
                };

                // 绘制线段
                for i in 0..self.data.len() - 1 {
                    let x1 = i as f64;
                    let y1 = self.data[i];
                    let x2 = (i + 1) as f64;
                    let y2 = self.data[i + 1];

                    // 将每个线段细分为多个小线段以实现平滑渐变
                    let segments = 10; // 细分数量
                    for j in 0..segments {
                        let t1 = j as f64 / segments as f64;
                        let t2 = (j + 1) as f64 / segments as f64;

                        let segment_x1 = x1 + (x2 - x1) * t1;
                        let segment_y1 = y1 + (y2 - y1) * t1;
                        let segment_x2 = x1 + (x2 - x1) * t2;
                        let segment_y2 = y1 + (y2 - y1) * t2;

                        // 使用细分段中点的颜色
                        let segment_y_mid = (segment_y1 + segment_y2) / 2.0;
                        let color = color_at(segment_y_mid);

                        let line = egui_plot::Line::new(PlotPoints::from(vec![
                            [segment_x1, segment_y1],
                            [segment_x2, segment_y2],
                        ]))
                        .color(color)
                        .width(3.0);

                        plot_ui.line(line);
                    }
                }

                // 添加数据点标记
                for (i, &y) in self.data.iter().enumerate() {
                    let x = i as f64;
                    let color = color_at(y);

                    let point = egui_plot::Points::new(PlotPoints::from(vec![[x, y]]))
                        .color(color)
                        .radius(4.0)
                        .shape(egui_plot::MarkerShape::Circle);

                    plot_ui.points(point);
                }
            });
        });
    }

    #[allow(dead_code)]
    fn draw_histogram(&mut self, ui: &mut egui::Ui) {
        let reset_view = ui.small_button("R").clicked();

        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            let available_width = ui.available_width();
            let plot_height = ui.available_height().max(MyEguiApp::DESIRED_HEIGHT);
            let plot_width = (10.0 * plot_height).min(available_width * 0.5);
            ui.add_space(available_width - plot_width - 2.);
            let mut plot = Plot::new("Histogram")
                .include_y(0.)
                .include_y(1.)
                .allow_zoom(false)
                .x_axis_formatter(|_, _| String::new())
                .y_axis_formatter(|_, _| String::new())
                .width(plot_width)
                .height(plot_height);
            if reset_view {
                plot = plot.reset();
            }

            let bar_width = 0.90;
            let bars: Vec<Bar> = self
                .data
                .iter()
                .enumerate()
                .map(|(i, &value)| Bar::new(i as f64, value).width(bar_width.into()))
                .collect();
            let chart = BarChart::new(bars).color(egui::Color32::from_rgb(100, 150, 200));
            plot.show(ui, |plot_ui| {
                plot_ui.bar_chart(chart);
            });
        });
    }

    #[allow(dead_code)]
    fn draw_cosine_curve(&mut self, ui: &mut egui::Ui) {
        let reset_view = ui.small_button("R").clicked();

        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            let available_width = ui.available_width();
            let plot_height = ui.available_height().max(MyEguiApp::DESIRED_HEIGHT);
            let plot_width = (10.0 * plot_height).min(available_width * 0.5);
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
    }
}

impl eframe::App for MyEguiApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
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
        let mut ltsymbol = String::from(" Nan ");
        if let Some(message) = self.message.as_ref() {
            self.visible = message.monitor_info.showbar0;
            ltsymbol = message.monitor_info.ltsymbol.clone();
            let monitor_width = message.monitor_info.monitor_width as f32;
            let desired_width = monitor_width;
            let desired_size =
                egui::Vec2::new(desired_width / scale_factor, MyEguiApp::DESIRED_HEIGHT);
            // No need to care about height
            if desired_size.x != screen_rect.size().x {
                // let size_log_info = format!(
                //     "desired_size: {}, screen_rect: {};",
                //     desired_size,
                //     screen_rect.size()
                // );
                // ui.label(&size_log_info);
                // println!("{}", size_log_info);
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::Pos2::ZERO));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(desired_size));
            }
        }

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
                ui.label(egui::RichText::new(ltsymbol).color(Color32::from_rgb(255, 0, 0)));
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
                    self.sys.refresh_cpu_all();
                    self.data.clear();
                    for (_, cpu) in self.sys.cpus().iter().enumerate() {
                        // println!("{}: {}%", i, cpu.cpu_usage());
                        self.data.push((cpu.cpu_usage() / 100.).into());
                    }
                    let unavailable =
                        (self.sys.total_memory() - self.sys.available_memory()) as f64 / 1e9;
                    ui.label(
                        egui::RichText::new(format!("{:.1}", unavailable)).color(MyEguiApp::SILVER),
                    );
                    let available = self.sys.available_memory() as f64 / 1e9;
                    ui.label(
                        egui::RichText::new(format!("{:.1}", available)).color(MyEguiApp::CYAN),
                    );
                    self.draw_smooth_gradient_line(ui);
                    // self.draw_histogram(ui);
                    // self.draw_cosine_curve(ui);
                });
            });
        });
    }
}
