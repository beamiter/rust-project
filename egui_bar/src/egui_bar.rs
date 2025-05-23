use eframe::egui;
use egui::{Align, Color32, Layout};
use egui_plot::{Line, Plot, PlotPoints};
use log::info;
use shared_structures::SharedMessage;
use std::{f64::consts::PI, process::Command, sync::mpsc, time::Instant};
use sysinfo::System;

// 将颜色常量移到单独的模块中，提高代码组织性
pub mod constants {
    use egui::Color32;

    pub const FONT_SIZE: f32 = 16.0;
    pub const DESIRED_HEIGHT: f32 = FONT_SIZE + 18.0;
    pub const VOLUME_WINDOW_HEIGHT: f32 = 300.0; // 音量控制窗口的高度

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

use constants::{
    colors, DESIRED_HEIGHT, FONT_SIZE, NUM_EMOJI_VEC, TAG_ICONS, VOLUME_WINDOW_HEIGHT,
};

// 音量控制窗口的状态
struct VolumeControlWindow {
    open: bool,
    master_volume: i32,
    headphone_volume: i32,
    speaker_volume: i32,
    microphone_volume: i32,
    is_muted: bool,
    selected_device: usize,
    available_devices: Vec<String>,
    position: Option<egui::Pos2>, // 存储窗口位置
}

impl Default for VolumeControlWindow {
    fn default() -> Self {
        Self {
            open: false,
            master_volume: 50,
            headphone_volume: 50,
            speaker_volume: 50,
            microphone_volume: 50,
            is_muted: false,
            selected_device: 0,
            available_devices: vec!["Default".to_string()],
            position: None,
        }
    }
}

#[allow(unused)]
pub struct MyEguiApp {
    message: Option<SharedMessage>,
    receiver_msg: mpsc::Receiver<SharedMessage>,
    sender_resize: mpsc::Sender<bool>,
    sys: System,
    point_index: usize,
    points: Vec<[f64; 2]>,
    point_speed: usize,
    toggle_time_style: bool,
    data: Vec<f64>,
    // 添加缓存和状态变量
    color_cache: Vec<Color32>,
    last_update_time: Instant,
    update_interval_ms: u64,
    // 音量控制窗口
    volume_window: VolumeControlWindow,
    // 窗口大小调整状态
    need_resize: bool,
    current_window_height: f32,
}

impl MyEguiApp {
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        receiver_msg: mpsc::Receiver<SharedMessage>,
        sender_resize: mpsc::Sender<bool>,
    ) -> Self {
        // 预计算余弦点，避免在构造函数中重复计算
        let points = Self::generate_cosine_points();

        // 初始化音量控制窗口
        let mut volume_window = VolumeControlWindow::default();
        volume_window.is_muted = Self::is_master_muted();
        volume_window.master_volume = Self::get_current_volume();
        volume_window.available_devices = Self::get_audio_devices();

        Self {
            message: None,
            receiver_msg,
            sender_resize,
            sys: System::new_all(),
            point_index: 0,
            points,
            point_speed: 2,
            toggle_time_style: false,
            data: Vec::with_capacity(16), // 预分配容量
            color_cache: Vec::new(),
            last_update_time: Instant::now(),
            update_interval_ms: 500, // 更新间隔，可调整
            volume_window,
            need_resize: false,
            current_window_height: DESIRED_HEIGHT,
        }
    }

    /// 获取 Master 输出的当前静音状态。
    /// 此函数执行 `amixer get Master` 并解析其输出。
    /// 它假设如果 Master 通道被静音，输出将包含 `[off]` 字符串。
    /// # 返回
    /// - `true` 如果 Master 输出被静音。
    /// - `false` 如果 Master 输出未被静音，或者无法确定状态（例如命令执行失败或 `[off]` 未找到）。
    fn is_master_muted() -> bool {
        // 尝试使用 amixer 获取 Master 通道的当前状态
        match Command::new("amixer").args(["get", "Master"]).output() {
            Ok(output) => {
                // 检查 amixer 命令是否成功执行
                if !output.status.success() {
                    // 如果命令本身失败（例如 amixer 未找到，或执行出错），打印错误并返回默认值
                    // eprintln!("amixer command failed with status: {}", output.status);
                    return false; // 默认未静音
                }
                let output_str = String::from_utf8_lossy(&output.stdout);
                // 在 amixer 的输出中，静音的通道通常会显示 `[off]`。
                // 例如: "Front Left: Playback 0 [0%] [-infdB] [off]"
                // 我们直接查找是否存在 "[off]" 这个子字符串。
                // 这是一个相对简单的检查，但对于典型的 ALSA 和 amixer 设置是有效的。
                // 如果 Master 通道被静音，其状态描述中应包含 "[off]"。
                // 如果 Master 通道没有静音能力（即没有 pswitch），则不会有 "[on]" 或 "[off]"，
                // 这种情况下 .contains("[off]") 会返回 false，这也是期望的行为（因为它没有被静音）。
                if output_str.contains("[off]") {
                    true // 找到了 "[off]"，表示已静音
                } else {
                    false // 未找到 "[off]"，表示未静音 (或者没有静音开关)
                }
            }
            Err(_e) => {
                // 如果执行 amixer 命令本身失败（例如，进程无法启动）
                // eprintln!("Failed to execute amixer command: {}", _e);
                false // 发生错误，默认未静音
            }
        }
    }

    // 获取当前系统音量
    fn get_current_volume() -> i32 {
        // 尝试使用 amixer 获取当前音量
        match Command::new("amixer").args(["get", "Master"]).output() {
            Ok(output) => {
                let output_str = String::from_utf8_lossy(&output.stdout);
                // 解析输出以获取音量百分比
                if let Some(percent_pos) = output_str.find('%') {
                    if let Some(start_pos) = output_str[..percent_pos].rfind('[') {
                        if let Ok(volume) = output_str[start_pos + 1..percent_pos].parse::<i32>() {
                            return volume;
                        }
                    }
                }
                50 // 默认值
            }
            Err(_) => 50, // 如果失败，则返回默认值
        }
    }

    // 获取可用的音频设备
    fn get_audio_devices() -> Vec<String> {
        let mut devices = vec!["Master".to_string()];

        // 尝试获取音频设备列表
        match Command::new("aplay").arg("-l").output() {
            Ok(output) => {
                let output_str = String::from_utf8_lossy(&output.stdout);
                for line in output_str.lines() {
                    if line.starts_with("card ") {
                        if let Some(device_name) = line.split(':').nth(1) {
                            devices.push(device_name.trim().to_string());
                        }
                    }
                }
            }
            Err(_) => {}
        }

        // 添加一些常见的控制项
        devices.push("Headphone".to_string());
        devices.push("Speaker".to_string());
        devices.push("Microphone".to_string());

        devices
    }

    // 设置系统音量
    fn set_volume(&mut self, device: &str, volume: i32, mute: bool) {
        // 使用 amixer 设置音量
        let _ = Command::new("amixer")
            .args([
                "set",
                device,
                &format!("{}%", volume),
                if mute { "mute" } else { "unmute" },
            ])
            .spawn();

        // 更新对应的音量设置
        match device {
            "Master" => self.volume_window.master_volume = volume,
            "Headphone" => self.volume_window.headphone_volume = volume,
            "Speaker" => self.volume_window.speaker_volume = volume,
            "Microphone" => self.volume_window.microphone_volume = volume,
            _ => {}
        }
    }

    // 打开 alsamixer
    fn open_alsamixer(&self) {
        // 在终端中打开 alsamixer
        let _ = Command::new("terminator").args(["-e", "alsamixer"]).spawn();
    }

    // 绘制音量按钮
    fn draw_volume_button(&mut self, ui: &mut egui::Ui) {
        let volume_icon = if self.volume_window.is_muted || self.volume_window.master_volume == 0 {
            "🔇" // 静音
        } else if self.volume_window.master_volume < 30 {
            "🔈" // 低音量
        } else if self.volume_window.master_volume < 70 {
            "🔉" // 中音量
        } else {
            "🔊" // 高音量
        };

        if ui.button(volume_icon).clicked() {
            // 切换音量窗口状态
            self.volume_window.open = !self.volume_window.open;

            // 标记需要调整窗口大小
            self.need_resize = true;
        }
    }

    // 绘制音量控制窗口
    fn draw_volume_window(&mut self, ctx: &egui::Context) -> bool {
        if !self.volume_window.open {
            return false;
        }

        // 不再使用 open 参数，而是在窗口内部跟踪关闭操作
        let mut window_closed = false;

        egui::Window::new("音量控制")
            .collapsible(false)
            .resizable(false)
            .default_width(300.0)
            .default_pos(self.volume_window.position.unwrap_or_else(|| {
                // 如果没有保存位置，设置为屏幕中央
                let screen_rect = ctx.screen_rect();
                egui::pos2(
                    screen_rect.center().x - 150.0,
                    screen_rect.center().y - 150.0,
                )
            }))
            // 移除 .open() 调用
            .show(ctx, |ui| {
                // 保存窗口位置
                if let Some(response) = ui.ctx().memory(|mem| mem.area_rect(ui.id())) {
                    self.volume_window.position = Some(response.left_top());
                }

                // 设备选择下拉菜单
                egui::ComboBox::from_label("设备")
                    .selected_text(
                        &self.volume_window.available_devices[self.volume_window.selected_device],
                    )
                    .show_ui(ui, |ui| {
                        for (idx, device) in self.volume_window.available_devices.iter().enumerate()
                        {
                            ui.selectable_value(
                                &mut self.volume_window.selected_device,
                                idx,
                                device,
                            );
                        }
                    });

                ui.add_space(10.0);

                // 主音量控制
                ui.horizontal(|ui| {
                    ui.label("主音量:");
                    if ui
                        .button(if self.volume_window.is_muted {
                            "🔇"
                        } else {
                            "🔊"
                        })
                        .clicked()
                    {
                        self.volume_window.is_muted = !self.volume_window.is_muted;
                        self.set_volume(
                            "Master",
                            self.volume_window.master_volume,
                            self.volume_window.is_muted,
                        );
                    }
                });

                let mut master_volume = self.volume_window.master_volume;
                if ui
                    .add(egui::Slider::new(&mut master_volume, 0..=100).text("音量"))
                    .changed()
                {
                    self.volume_window.master_volume = master_volume;
                    self.set_volume("Master", master_volume, self.volume_window.is_muted);
                }

                ui.add_space(10.0);

                // 根据选择的设备显示不同的控制选项
                match self.volume_window.selected_device {
                    0 => {
                        // 主设备 - 显示所有控制
                        ui.collapsing("高级控制", |ui| {
                            // 耳机音量
                            let mut headphone_volume = self.volume_window.headphone_volume;
                            if ui
                                .add(egui::Slider::new(&mut headphone_volume, 0..=100).text("耳机"))
                                .changed()
                            {
                                self.volume_window.headphone_volume = headphone_volume;
                                self.set_volume("Headphone", headphone_volume, false);
                            }

                            // 扬声器音量
                            let mut speaker_volume = self.volume_window.speaker_volume;
                            if ui
                                .add(egui::Slider::new(&mut speaker_volume, 0..=100).text("扬声器"))
                                .changed()
                            {
                                self.volume_window.speaker_volume = speaker_volume;
                                self.set_volume("Speaker", speaker_volume, false);
                            }

                            // 麦克风音量
                            let mut microphone_volume = self.volume_window.microphone_volume;
                            if ui
                                .add(
                                    egui::Slider::new(&mut microphone_volume, 0..=100)
                                        .text("麦克风"),
                                )
                                .changed()
                            {
                                self.volume_window.microphone_volume = microphone_volume;
                                self.set_volume("Capture", microphone_volume, false);
                            }
                        });
                    }
                    _ => {
                        // 特定设备控制
                        let device_name = &self.volume_window.available_devices
                            [self.volume_window.selected_device]
                            .clone();
                        let mut device_volume = 50; // 默认值，实际应用中应该获取当前值
                        if ui
                            .add(egui::Slider::new(&mut device_volume, 0..=100).text(device_name))
                            .changed()
                        {
                            self.set_volume(device_name, device_volume, false);
                        }
                    }
                }

                ui.add_space(10.0);

                // 按钮区域
                ui.horizontal(|ui| {
                    if ui.button("高级混音器").clicked() {
                        self.open_alsamixer();
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::RIGHT), |ui| {
                        if ui.button("关闭").clicked() {
                            // 不再直接修改 window_open，而是设置我们自己的标志
                            window_closed = true;
                        }
                    });
                });
            });

        // 检查窗口是否应该关闭
        // 如果用户点击了关闭按钮或者窗口被系统关闭
        if window_closed || ctx.input(|i| i.viewport().close_requested()) {
            self.volume_window.open = false;
            self.need_resize = true;
            return true; // 窗口状态已改变
        }

        false // 窗口状态未改变
    }

    // 计算当前应使用的窗口高度
    fn calculate_window_height(&self) -> f32 {
        if self.volume_window.open {
            // 当音量控制窗口打开时使用更大高度
            VOLUME_WINDOW_HEIGHT
        } else {
            // 否则使用默认紧凑高度
            DESIRED_HEIGHT
        }
    }

    // 调整窗口大小
    fn adjust_window_size(&mut self, ctx: &egui::Context, scale_factor: f32, monitor_width: f32) {
        // 计算应使用的高度
        let target_height = self.calculate_window_height();
        let screen_rect = ctx.screen_rect();
        let desired_width = monitor_width;
        let desired_size = egui::Vec2::new(desired_width / scale_factor, target_height);

        // 如果高度发生变化或被标记为需要调整大小
        if self.need_resize
            || (target_height - self.current_window_height).abs() > 1.0
            || (desired_size.x != screen_rect.size().x)
        {
            // 调整窗口大小
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::Pos2::ZERO));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(desired_size));

            // 更新当前高度和调整状态
            self.current_window_height = target_height;
        }
    }

    // 将点生成提取为单独函数
    fn generate_cosine_points() -> Vec<[f64; 2]> {
        let step_num = 60;
        let step: f64 = PI / step_num as f64;
        (-step_num..=step_num)
            .map(|x| {
                let tmp_x = x as f64 * step;
                [tmp_x, tmp_x.cos()]
            })
            .collect()
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

    fn draw_smooth_gradient_line(&mut self, ui: &mut egui::Ui) {
        // 确保颜色缓存已初始化
        self.ensure_color_cache();

        let reset_view = ui.small_button("R").clicked();

        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            let available_width = ui.available_width();
            let plot_height = ui.available_height().max(DESIRED_HEIGHT);
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
                            .width(3.0);

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
                        .radius(4.0)
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
        // 处理消息
        if self.need_resize {
            let _output = Command::new("xsetroot")
                .arg("-name")
                .arg("revoke by egui_bar")
                .output();
            info!("try to revoke");
        }
        let prev_message = self.message.clone();
        while let Ok(message) = self.receiver_msg.try_recv() {
            self.message = Some(message);
            self.need_resize = true;
        }
        if let Some(prev_message) = prev_message {
            if let Some(current_message) = &self.message {
                if (prev_message.timestamp != current_message.timestamp)
                    && prev_message.monitor_info == current_message.monitor_info
                {
                    self.need_resize = false;
                }
            }
        }

        // 更新系统信息（限制更新频率）
        self.update_system_info();

        // 绘制音量控制窗口（如果打开）
        // 如果窗口状态改变（例如关闭），标记需要调整大小
        if self.draw_volume_window(ctx) {
            self.need_resize = true;
        }

        let scale_factor = ctx.pixels_per_point();
        // 处理窗口大小调整
        if let Some(message) = self.message.as_ref() {
            let monitor_width = message.monitor_info.monitor_width as f32;

            // 调整窗口大小，考虑音量控制窗口的状态
            self.adjust_window_size(ctx, scale_factor, monitor_width);
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
                    let mut rich_text =
                        egui::RichText::new(tag_icon).font(egui::FontId::monospace(FONT_SIZE));

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
                    if ui.small_button(format!("ⓢ {:.2}", scale_factor)).clicked() {
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
                    self.draw_smooth_gradient_line(ui);
                });
            });
        });
        if self.need_resize {
            ctx.request_repaint_after(std::time::Duration::from_micros(1));
        }
    }
}
