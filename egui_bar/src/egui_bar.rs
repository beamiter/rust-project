use eframe::egui;
use egui::Margin;
use egui::{Align, Color32, Layout};
use egui::{FontFamily, FontId, TextStyle};
use egui_plot::{Line, Plot, PlotPoints};
use log::info;
use shared_structures::SharedMessage;
use std::collections::BTreeMap;
use std::sync::Once;
use std::{
    process::Command,
    sync::mpsc,
    time::{Duration, Instant},
};
use sysinfo::System;
use FontFamily::Monospace;
use FontFamily::Proportional;
static INIT: Once = Once::new();

// 将颜色常量移到单独的模块中，提高代码组织性
pub mod constants {
    use egui::Color32;

    pub const FONT_SIZE: f32 = 16.0;

    pub mod colors {
        use super::Color32;

        pub const RED: Color32 = Color32::from_rgb(255, 0, 0);
        pub const ORANGE: Color32 = Color32::from_rgb(255, 127, 0);
        pub const YELLOW: Color32 = Color32::from_rgb(255, 255, 0);
        pub const GREEN: Color32 = Color32::from_rgb(0, 255, 0);
        pub const BLUE: Color32 = Color32::from_rgb(0, 0, 255);
        pub const INDIGO: Color32 = Color32::from_rgb(75, 0, 130);
        pub const VIOLET: Color32 = Color32::from_rgb(148, 0, 211);
        pub const BROWN: Color32 = Color32::from_rgb(165, 42, 42);
        // pub const GOLD: Color32 = Color32::from_rgb(255, 215, 0);
        // pub const MAGENTA: Color32 = Color32::from_rgb(255, 0, 255);
        pub const CYAN: Color32 = Color32::from_rgb(0, 255, 255);
        pub const SILVER: Color32 = Color32::from_rgb(192, 192, 192);
        // pub const OLIVE_GREEN: Color32 = Color32::from_rgb(128, 128, 0);
        // pub const ROYALBLUE: Color32 = Color32::from_rgb(65, 105, 225);
        pub const WHEAT: Color32 = Color32::from_rgb(245, 222, 179);

        pub const TAG_COLORS: [Color32; 9] = [
            RED, ORANGE, YELLOW, GREEN, BLUE, INDIGO, VIOLET, BROWN, CYAN,
        ];
    }

    pub const TAG_ICONS: [&str; 9] = [
        " 🍟 ", " 😃 ", " 🚀 ", " 🎉 ", " 🍕 ", " 🍖 ", " 🏍 ", " 🍔 ", " 🍘 ",
    ];

    pub const NUM_EMOJI_VEC: [&str; 2] = ["⓪", "①"];
}

use constants::{colors, FONT_SIZE, NUM_EMOJI_VEC, TAG_ICONS};

use crate::audio_manager::AudioManager;

// 音量控制窗口的状态
struct VolumeControlWindow {
    open: bool,
    selected_device: usize,
    position: Option<egui::Pos2>,
    last_volume_change: Instant,
    volume_change_debounce: Duration,
}

impl Default for VolumeControlWindow {
    fn default() -> Self {
        Self {
            open: false,
            selected_device: 0,
            position: None,
            last_volume_change: Instant::now(),
            volume_change_debounce: Duration::from_millis(50), // 防抖间隔
        }
    }
}

#[allow(dead_code)]
pub struct MyEguiApp {
    // 保留原有字段...
    message: Option<SharedMessage>,
    receiver_msg: mpsc::Receiver<SharedMessage>,
    sender_resize: mpsc::Sender<bool>,
    sys: System,
    toggle_time_style: bool,
    data: Vec<f64>,
    color_cache: Vec<Color32>,
    last_update_time: Instant,
    update_interval_ms: u64,
    volume_window: VolumeControlWindow,
    need_resize: bool,
    current_window_height: f32,
    scale_factor: f32,

    // 添加音频管理器
    audio_manager: AudioManager,
}

impl MyEguiApp {
    pub fn new(
        _: &eframe::CreationContext<'_>,
        receiver_msg: mpsc::Receiver<SharedMessage>,
        sender_resize: mpsc::Sender<bool>,
    ) -> Self {
        // 初始化音频管理器
        let audio_manager = AudioManager::new();

        Self {
            message: None,
            receiver_msg,
            sender_resize,
            sys: System::new_all(),
            toggle_time_style: false,
            data: Vec::with_capacity(16),
            color_cache: Vec::new(),
            last_update_time: Instant::now(),
            update_interval_ms: 500,
            volume_window: VolumeControlWindow::default(),
            need_resize: false,
            current_window_height: FONT_SIZE * 2.0,
            scale_factor: 1.0,
            audio_manager,
        }
    }

    pub fn configure_text_styles(ctx: &egui::Context, font_scale_factor: f32) {
        ctx.all_styles_mut(move |style| {
            let scaled_font_size = FONT_SIZE / font_scale_factor;
            info!(
                "[configure_text_styles] scaled_font_size: {}",
                scaled_font_size
            );
            let text_styles: BTreeMap<TextStyle, FontId> = [
                (TextStyle::Body, FontId::new(scaled_font_size, Monospace)),
                (
                    TextStyle::Monospace,
                    FontId::new(scaled_font_size, Monospace),
                ),
                (TextStyle::Button, FontId::new(scaled_font_size, Monospace)),
                (
                    TextStyle::Small,
                    FontId::new(scaled_font_size / 2., Proportional),
                ),
                (
                    TextStyle::Heading,
                    FontId::new(scaled_font_size * 2., Proportional),
                ),
            ]
            .into();
            style.text_styles = text_styles;
            style.spacing.window_margin = Margin::same(0.0);
            style.spacing.menu_spacing = 0.0;
            style.spacing.menu_margin = Margin::same(0.0);
        });
    }

    // 绘制音量按钮
    fn draw_volume_button(&mut self, ui: &mut egui::Ui) {
        // 获取主音量设备状态
        let (volume, is_muted) = if let Some(device) = self.audio_manager.get_master_device() {
            (device.volume, device.is_muted)
        } else {
            (50, false) // 默认值
        };

        // 根据音量和静音状态选择图标
        let volume_icon = if is_muted || volume == 0 {
            "🔇" // 静音
        } else if volume < 30 {
            "🔈" // 低音量
        } else if volume < 70 {
            "🔉" // 中音量
        } else {
            "🔊" // 高音量
        };

        // 点击按钮打开/关闭音量控制窗口
        let response = ui.button(volume_icon);
        if response.clicked() {
            self.volume_window.open = !self.volume_window.open;
            self.need_resize = true;
            self.audio_manager.refresh_devices().ok();
        }

        // 正确的悬停文本用法
        if let Some(device) = self.audio_manager.get_master_device() {
            response.on_hover_text(format!(
                "{}：{}%{}",
                device.description,
                device.volume,
                if device.is_muted { " (已静音)" } else { "" }
            ));
        }
    }

    #[allow(dead_code)]
    fn set_volume(&mut self, device: &str, volume: i32, mute: bool) {
        if let Err(e) = self.audio_manager.set_volume(device, volume, mute) {
            eprintln!("Failed to set volume: {}", e);
        }
    }

    #[allow(dead_code)]
    fn is_master_muted(&self) -> bool {
        self.audio_manager
            .get_master_device()
            .map(|device| device.is_muted)
            .unwrap_or(false)
    }

    #[allow(dead_code)]
    fn get_current_volume(&self) -> i32 {
        self.audio_manager
            .get_master_device()
            .map(|device| device.volume)
            .unwrap_or(50)
    }

    #[allow(dead_code)]
    fn get_audio_devices(&self) -> Vec<String> {
        self.audio_manager
            .get_devices()
            .iter()
            .map(|device| device.name.clone())
            .collect()
    }

    // 打开 alsamixer 功能保持不变
    fn open_alsamixer(&self) {
        let _ = Command::new("terminator").args(["-e", "alsamixer"]).spawn();
    }

    // 绘制音量控制窗口
    fn draw_volume_window(&mut self, ctx: &egui::Context) -> bool {
        if !self.volume_window.open {
            return false;
        }

        // 在每一帧更新音频设备状态
        self.audio_manager.update_if_needed();

        let mut window_closed = false;

        egui::Window::new("音量控制")
            .collapsible(false)
            .resizable(false)
            .default_width(300.0)
            .default_pos(self.volume_window.position.unwrap_or_else(|| {
                let screen_rect = ctx.screen_rect();
                egui::pos2(
                    screen_rect.center().x - 150.0,
                    screen_rect.center().y - 150.0,
                )
            }))
            .show(ctx, |ui| {
                // 保存窗口位置
                if let Some(response) = ui.ctx().memory(|mem| mem.area_rect(ui.id())) {
                    self.volume_window.position = Some(response.left_top());
                }

                // 获取所有可用设备
                let devices = self.audio_manager.get_devices();

                if devices.is_empty() {
                    ui.label("没有找到可控制的音频设备");
                    return;
                }

                // 设备选择下拉菜单
                let device_names: Vec<(usize, String)> = devices
                    .iter()
                    .enumerate()
                    .filter(|(_, d)| d.has_volume_control || d.has_switch_control)
                    .map(|(i, d)| (i, d.description.clone()))
                    .collect();

                if !device_names.is_empty() {
                    // 确保选中的设备索引有效
                    if self.volume_window.selected_device >= device_names.len() {
                        self.volume_window.selected_device = 0;
                    }

                    ui.horizontal(|ui| {
                        ui.label("设备：");
                        egui::ComboBox::from_id_salt("audio_device_selector")
                            .selected_text(&device_names[self.volume_window.selected_device].1)
                            .width(200.0)
                            .show_ui(ui, |ui| {
                                for (idx, (_dev_idx, name)) in device_names.iter().enumerate() {
                                    if ui
                                        .selectable_label(
                                            self.volume_window.selected_device == idx,
                                            name,
                                        )
                                        .clicked()
                                    {
                                        self.volume_window.selected_device = idx;
                                    }
                                }
                            });
                    });

                    ui.add_space(10.0);

                    // 获取选中的设备索引
                    if let Some(&(device_idx, _)) =
                        device_names.get(self.volume_window.selected_device)
                    {
                        let device_data =
                            { self.audio_manager.get_device_by_index(device_idx).clone() };
                        if let Some(device_data_from_manager) = device_data {
                            let device_name_clone = device_data_from_manager.name.clone(); // String, so clone
                            let mut current_volume_copy = device_data_from_manager.volume; // Assuming Copy type (e.g., i64, f32)
                            let is_muted_copy = device_data_from_manager.is_muted;
                            let has_switch_control_copy =
                                device_data_from_manager.has_switch_control;
                            // 绘制音量控制器
                            if device_data_from_manager.has_volume_control {
                                ui.horizontal(|ui| {
                                    ui.label("音量：");

                                    // 静音按钮
                                    if has_switch_control_copy {
                                        let mute_btn =
                                            ui.button(if is_muted_copy { "🔇" } else { "🔊" });
                                        if mute_btn.clicked() {
                                            if let Err(e) =
                                                self.audio_manager.toggle_mute(&device_name_clone)
                                            {
                                                eprintln!("Failed to toggle mute: {}", e);
                                            }
                                        }
                                        mute_btn.on_hover_text(if is_muted_copy {
                                            "取消静音"
                                        } else {
                                            "静音"
                                        });
                                    }

                                    // 显示当前音量百分比
                                    ui.label(format!("{}%", current_volume_copy));
                                });

                                // 音量滑块
                                if ui
                                    .add(
                                        egui::Slider::new(&mut current_volume_copy, 0..=100)
                                            .show_value(false)
                                            .text(""),
                                    )
                                    .changed()
                                {
                                    // 防抖动
                                    let now = Instant::now();
                                    if now.duration_since(self.volume_window.last_volume_change)
                                        > self.volume_window.volume_change_debounce
                                    {
                                        self.volume_window.last_volume_change = now;
                                        if let Err(e) = self.audio_manager.set_volume(
                                            &device_name_clone,
                                            current_volume_copy,
                                            is_muted_copy,
                                        ) {
                                            eprintln!("Failed to set volume: {}", e);
                                        }
                                    }
                                }
                            } else if device_data_from_manager.has_switch_control {
                                // 只有开关控制的设备
                                ui.horizontal(|ui| {
                                    let btn = ui.button(if is_muted_copy {
                                        "◉ 已禁用"
                                    } else {
                                        "◎ 已启用"
                                    });

                                    if btn.clicked() {
                                        if let Err(e) =
                                            self.audio_manager.toggle_mute(&device_name_clone)
                                        {
                                            eprintln!("Failed to toggle switch: {}", e);
                                        }
                                    }
                                });
                            } else {
                                ui.label("此设备没有可用的控制选项");
                            }
                        }
                    }
                }

                ui.add_space(10.0);

                // 添加按钮区域
                ui.horizontal(|ui| {
                    if ui.button("高级混音器").clicked() {
                        self.open_alsamixer();
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::RIGHT), |ui| {
                        if ui.button("关闭").clicked() {
                            window_closed = true;
                        }
                    });
                });
            });

        // 检查窗口是否应该关闭
        if window_closed || ctx.input(|i| i.viewport().close_requested()) {
            self.volume_window.open = false;
            self.need_resize = true;
            return true; // 窗口状态已改变
        }

        false
    }

    // 计算当前应使用的窗口高度
    fn calculate_window_height(&self, monitor_height: i32) -> f32 {
        if self.volume_window.open {
            // 当音量控制窗口打开时使用更大高度
            monitor_height as f32 * 0.3
        } else {
            // 否则使用默认紧凑高度
            monitor_height as f32 * 0.03
        }
    }

    // 调整窗口大小
    fn adjust_window_size(&mut self, ctx: &egui::Context, message: &SharedMessage) {
        // 计算应使用的高度
        let monitor_height = message.monitor_info.monitor_height;
        let target_height_raw = self.calculate_window_height(monitor_height);
        let screen_rect = ctx.screen_rect();
        let border_w = message.monitor_info.border_w as f32;
        let monitor_x = message.monitor_info.monitor_x as f32;
        let monitor_y = message.monitor_info.monitor_y as f32;
        let target_width_raw = message.monitor_info.monitor_width as f32 - 2. * border_w;
        let target_size = egui::Vec2::new(
            target_width_raw / self.scale_factor,
            target_height_raw / self.scale_factor,
        );
        let height_offset = ((target_height_raw - target_size.y) * 0.5).max(0.0);

        // 如果高度发生变化或被标记为需要调整大小
        if self.need_resize
            || (target_size.y - self.current_window_height).abs() > 2.0
            || (target_size.x - screen_rect.size().x).abs() > 2.0
        {
            let outer_pos = egui::Pos2::new(
                (monitor_x + border_w) / self.scale_factor,
                (monitor_y + border_w * 0.5) / self.scale_factor + height_offset,
            );
            // 调整窗口大小
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(outer_pos));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(target_size));
            info!("outer_pos: {}", outer_pos);
            info!("target_size: {}", target_size);
            info!("screen_rect: {}", screen_rect);
            info!("scale_factor: {}", self.scale_factor);

            // 更新当前高度和调整状态
            self.current_window_height = target_size.y;
        }
    }

    // 颜色映射函数
    fn color_at(&self, y: f64) -> Color32 {
        let y = y.clamp(0.0, 1.0) as f32;

        // 使用查表法，如果已经缓存了颜色，直接返回
        let index = (y * 100.0) as usize;
        if !self.color_cache.is_empty() && index < self.color_cache.len() {
            return self.color_cache[index];
        }

        // 否则计算颜色
        if y < 0.2 {
            // 蓝色到青色
            let t = y / 0.2;
            Color32::from_rgb(0, (t * 255.0) as u8, 255)
        } else if y < 0.4 {
            // 青色到绿色
            let t = (y - 0.2) / 0.2;
            Color32::from_rgb(0, 255, ((1.0 - t) * 255.0) as u8)
        } else if y < 0.6 {
            // 绿色到黄色
            let t = (y - 0.4) / 0.2;
            Color32::from_rgb((t * 255.0) as u8, 255, 0)
        } else if y < 0.8 {
            // 黄色到橙色
            let t = (y - 0.6) / 0.2;
            Color32::from_rgb(255, (255.0 * (1.0 - t * 0.5)) as u8, 0)
        } else {
            // 橙色到红色
            let t = (y - 0.8) / 0.2;
            Color32::from_rgb(255, (128.0 * (1.0 - t)) as u8, 0)
        }
    }

    // 初始化颜色缓存
    fn ensure_color_cache(&mut self) {
        if self.color_cache.is_empty() {
            self.color_cache = (0..=100)
                .map(|i| {
                    let y = i as f64 / 100.0;
                    self.color_at(y)
                })
                .collect();
        }
    }

    fn draw_smooth_gradient_line(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // 确保颜色缓存已初始化
        self.ensure_color_cache();
        let reset_view = ui.small_button("R").clicked();

        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            let available_width = ui.available_width();
            let screen_rect = ctx.screen_rect();
            let plot_height = ui.available_height().max(screen_rect.height());
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

                // 优化：预先分配内存
                let segments = 10;
                let mut line_points = Vec::with_capacity(2);

                // 绘制线段
                for i in 0..self.data.len() - 1 {
                    let x1 = i as f64;
                    let y1 = self.data[i];
                    let x2 = (i + 1) as f64;
                    let y2 = self.data[i + 1];

                    // 将每个线段细分为多个小线段以实现平滑渐变
                    for j in 0..segments {
                        let t1 = j as f64 / segments as f64;
                        let t2 = (j + 1) as f64 / segments as f64;

                        let segment_x1 = x1 + (x2 - x1) * t1;
                        let segment_y1 = y1 + (y2 - y1) * t1;
                        let segment_x2 = x1 + (x2 - x1) * t2;
                        let segment_y2 = y1 + (y2 - y1) * t2;

                        // 使用细分段中点的颜色
                        let segment_y_mid = (segment_y1 + segment_y2) / 2.0;
                        let index = (segment_y_mid * 100.0) as usize;
                        let color = self.color_cache[index.min(100)];

                        // 重用 line_points 向量
                        line_points.clear();
                        line_points.push([segment_x1, segment_y1]);
                        line_points.push([segment_x2, segment_y2]);

                        let line = Line::new(PlotPoints::from(line_points.clone()))
                            .color(color)
                            .width(1.0 / self.scale_factor);

                        plot_ui.line(line);
                    }
                }

                // 添加数据点标记
                for (i, &y) in self.data.iter().enumerate() {
                    let x = i as f64;
                    let index = (y * 100.0) as usize;
                    let color = self.color_cache[index.min(100)];

                    let point = egui_plot::Points::new(PlotPoints::from(vec![[x, y]]))
                        .color(color)
                        .radius(2.0 / self.scale_factor)
                        .shape(egui_plot::MarkerShape::Circle);

                    plot_ui.points(point);
                }
            });
        });
    }

    // 更新系统信息
    fn update_system_info(&mut self) {
        let now = Instant::now();
        if now.duration_since(self.last_update_time).as_millis() < self.update_interval_ms as u128 {
            return;
        }

        self.sys.refresh_memory();
        self.sys.refresh_cpu_all();
        self.last_update_time = now;

        // 更新CPU数据
        self.data.clear();
        self.data.reserve(self.sys.cpus().len());
        for cpu in self.sys.cpus() {
            self.data.push((cpu.cpu_usage() / 100.).into());
        }
    }
}

impl eframe::App for MyEguiApp {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.scale_factor = ctx.pixels_per_point();
        let new_scale_factor = self.scale_factor.min(1.1).max(1.0);
        if self.scale_factor != new_scale_factor {
            self.scale_factor = new_scale_factor;
            ctx.set_pixels_per_point(self.scale_factor);
            MyEguiApp::configure_text_styles(ctx, self.scale_factor);
            self.need_resize = true;
        } else {
            INIT.call_once(|| {
                info!("call_once, {}", self.scale_factor);
                self.need_resize = true;
                MyEguiApp::configure_text_styles(ctx, self.scale_factor);
            });
        }
        while let Ok(message) = self.receiver_msg.try_recv() {
            self.message = Some(message);
            self.need_resize = true;
        }

        // 更新系统信息（限制更新频率）
        self.update_system_info();

        // 绘制音量控制窗口（如果打开）
        // 如果窗口状态改变（例如关闭），标记需要调整大小
        if self.draw_volume_window(ctx) {
            self.need_resize = true;
        }

        // 处理窗口大小调整
        if let Some(ref message) = self.message.clone() {
            // 调整窗口大小，考虑音量控制窗口的状态
            self.adjust_window_size(ctx, message);
        }

        // 主UI面板
        egui::CentralPanel::default().show(ctx, |ui| {
            let mut tag_status_vec = Vec::new();
            let mut ltsymbol = String::from(" Nan ");

            if let Some(ref message) = self.message {
                tag_status_vec = message.monitor_info.tag_status_vec.clone();
                ltsymbol = message.monitor_info.ltsymbol.clone();
            }

            ui.horizontal_centered(|ui| {
                // 绘制标签图标
                for i in 0..TAG_ICONS.len() {
                    let tag_icon = TAG_ICONS[i];
                    let tag_color = colors::TAG_COLORS[i];
                    let mut rich_text = egui::RichText::new(tag_icon).monospace();

                    if let Some(ref tag_status) = tag_status_vec.get(i) {
                        if tag_status.is_selected {
                            rich_text = rich_text.underline();
                        }
                        if tag_status.is_filled {
                            rich_text = rich_text.strong().italics();
                        }
                        if tag_status.is_occ {
                            rich_text = rich_text.color(tag_color);
                        }
                        if tag_status.is_urg {
                            rich_text = rich_text.background_color(colors::WHEAT);
                        }
                    }
                    ui.label(rich_text);
                }

                ui.label(egui::RichText::new(ltsymbol).color(colors::RED));

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // 添加音量控制按钮 - 放在最右侧
                    self.draw_volume_button(ui);

                    // 时间显示
                    let current_time = chrono::Local::now()
                        .format(if self.toggle_time_style {
                            "%Y-%m-%d %H:%M:%S"
                        } else {
                            "%Y-%m-%d %H:%M"
                        })
                        .to_string();

                    if ui
                        .selectable_label(
                            true,
                            egui::RichText::new(current_time).color(colors::GREEN),
                        )
                        .clicked()
                    {
                        self.toggle_time_style = !self.toggle_time_style;
                    }

                    // 截图按钮
                    if ui
                        .small_button(format!("ⓢ {:.2}", self.scale_factor))
                        .clicked()
                    {
                        let _ = Command::new("flameshot").arg("gui").spawn();
                    }

                    // 显示监视器编号
                    let monitor_num = self
                        .message
                        .as_ref()
                        .map_or(0, |m| m.monitor_info.monitor_num as usize)
                        .min(1); // 确保索引安全

                    ui.label(
                        egui::RichText::new(format!("[{}]", NUM_EMOJI_VEC[monitor_num])).strong(),
                    );

                    // 内存信息显示
                    let unavailable =
                        (self.sys.total_memory() - self.sys.available_memory()) as f64 / 1e9;
                    ui.label(
                        egui::RichText::new(format!("{:.1}", unavailable)).color(colors::SILVER),
                    );

                    let available = self.sys.available_memory() as f64 / 1e9;
                    ui.label(egui::RichText::new(format!("{:.1}", available)).color(colors::CYAN));

                    // 绘制图表
                    self.draw_smooth_gradient_line(ui, ctx);
                });
            });
        });
        if self.need_resize {
            ctx.request_repaint_after(std::time::Duration::from_micros(1));
            self.need_resize = false;
        }
    }
}
