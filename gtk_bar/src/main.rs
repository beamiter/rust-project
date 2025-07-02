use cairo::Context;
use chrono::Local;
use flexi_logger::{Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming};
use gdk4::prelude::*;
use gdk4_x11::x11::xlib::{XFlush, XMoveWindow};
use glib::timeout_add_local;
use gtk4::gio::{self};
use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box, Button, DrawingArea, Label, Orientation, ProgressBar,
    ScrolledWindow, glib,
};
use log::{error, info, warn};
use std::env;
use std::rc::Rc;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

mod audio_manager;
mod error;
mod system_monitor;

use audio_manager::AudioManager;
use error::AppError;
use shared_structures::{CommandType, SharedCommand, SharedMessage, SharedRingBuffer, TagStatus};
use system_monitor::SystemMonitor;

const STATUS_BAR_PREFIX: &str = "gtk_bar";

// 使用 Arc<Mutex<>> 来共享状态
type SharedAppState = Arc<Mutex<AppState>>;

struct AppState {
    // Application state
    active_tab: usize,
    layout_symbol: String,
    monitor_num: u8,
    show_seconds: bool,

    tag_status_vec: Vec<TagStatus>,

    // System components
    audio_manager: AudioManager,
    system_monitor: SystemMonitor,

    // Communication
    command_sender: Option<mpsc::Sender<SharedCommand>>,
    last_shared_message: Option<SharedMessage>,

    pending_messages: Vec<SharedMessage>,
}

struct TabBarApp {
    // GTK widgets
    window: ApplicationWindow,
    tab_buttons: Vec<Button>,
    layout_label: Label,
    time_label: Button,
    monitor_label: Label,
    memory_progress: ProgressBar,
    cpu_drawing_area: DrawingArea,

    underline_areas: Vec<DrawingArea>,

    // Shared state
    state: SharedAppState,
}

impl TabBarApp {
    fn new(app: &Application) -> Rc<Self> {
        let tabs = vec![
            "🍜".to_string(),
            "🎨".to_string(),
            "🍀".to_string(),
            "🧿".to_string(),
            "🌟".to_string(),
            "🐐".to_string(),
            "🏆".to_string(),
            "🕊️".to_string(),
            "🏡".to_string(),
        ];

        let _tab_colors = vec![
            (1.0, 0.42, 0.42),  // 红色
            (0.31, 0.80, 0.77), // 青色
            (0.27, 0.72, 0.82), // 蓝色
            (0.59, 0.81, 0.71), // 绿色
            (1.0, 0.79, 0.34),  // 黄色
            (1.0, 0.62, 0.95),  // 粉色
            (0.33, 0.63, 1.0),  // 淡蓝色
            (0.37, 0.15, 0.80), // 紫色
            (0.0, 0.82, 0.83),  // 青绿色
        ];

        // 创建共享状态
        let state = Arc::new(Mutex::new(AppState {
            active_tab: 0,
            layout_symbol: " ? ".to_string(),
            monitor_num: 0,
            show_seconds: false,
            tag_status_vec: Vec::new(),
            audio_manager: AudioManager::new(),
            system_monitor: SystemMonitor::new(10),
            command_sender: None,
            last_shared_message: None,
            pending_messages: Vec::new(),
        }));

        // 创建主窗口

        let window = ApplicationWindow::builder()
            .application(app)
            .title(STATUS_BAR_PREFIX)
            .default_width(1000)
            .default_height(50)
            .decorated(false)
            .resizable(true)
            .build();

        // 使用垂直 Box 作为主容器

        let main_vbox = Box::new(Orientation::Vertical, 2);

        main_vbox.set_margin_top(2);

        main_vbox.set_margin_bottom(2);

        main_vbox.set_margin_start(2);

        main_vbox.set_margin_end(2);

        // 第一行：主要内容区域

        let top_hbox = Box::new(Orientation::Horizontal, 3);

        // 左侧：Tab 按钮区域

        let tab_box = Box::new(Orientation::Horizontal, 3);

        let mut tab_buttons = Vec::new();

        for tab_text in &tabs {
            let button = Button::builder()
                .label(tab_text)
                .width_request(32)
                .height_request(32)
                .build();

            tab_box.append(&button);

            tab_buttons.push(button);
        }

        // 布局区域

        let layout_label = Label::new(Some(" ? "));

        layout_label.set_halign(gtk4::Align::Center);

        layout_label.set_width_request(40);

        layout_label.set_height_request(32);

        // 布局按钮容器

        let layout_box = Box::new(Orientation::Horizontal, 5);

        let layout_button_1 = Button::with_label("[]=");

        let layout_button_2 = Button::with_label("><>");

        let layout_button_3 = Button::with_label("[M]");

        layout_button_1.set_size_request(40, 32);

        layout_button_2.set_size_request(40, 32);

        layout_button_3.set_size_request(40, 32);

        layout_box.append(&layout_button_1);

        layout_box.append(&layout_button_2);

        layout_box.append(&layout_button_3);

        let layout_scroll = ScrolledWindow::new();

        layout_scroll.set_policy(gtk4::PolicyType::Automatic, gtk4::PolicyType::Never);

        layout_scroll.set_size_request(70, 32);

        layout_scroll.set_child(Some(&layout_box));

        // 中间：弹性空间

        let spacer = Box::new(Orientation::Horizontal, 0);

        spacer.set_hexpand(true); // 这个会占据所有剩余空间

        // 右侧：系统信息区域

        let right_box = Box::new(Orientation::Horizontal, 3);

        right_box.set_halign(gtk4::Align::End); // 确保对齐到右侧

        let cpu_drawing_area = DrawingArea::new();

        cpu_drawing_area.set_size_request(32, 32);

        let screenshot_button = Button::with_label(&format!(" s {:.2} ", 1.0));

        screenshot_button.set_size_request(60, 32);

        let time_label = Button::with_label("--:--");

        time_label.set_size_request(60, 32);

        let monitor_label = Label::new(Some("🥇"));

        monitor_label.set_size_request(30, 32);

        monitor_label.set_halign(gtk4::Align::Center);

        // 添加到右侧容器

        right_box.append(&cpu_drawing_area);

        right_box.append(&screenshot_button);

        right_box.append(&time_label);

        right_box.append(&monitor_label);

        // 组装顶部行

        top_hbox.append(&tab_box);

        top_hbox.append(&layout_label);

        top_hbox.append(&layout_scroll);

        top_hbox.append(&spacer); // 弹性空间

        top_hbox.append(&right_box); // 右侧组件

        // 第二行：下划线和进度条

        let bottom_hbox = Box::new(Orientation::Horizontal, 0);

        // 左侧：下划线区域

        let underline_box = Box::new(Orientation::Horizontal, 3);

        let mut underline_areas = Vec::new();

        for _ in &tabs {
            let underline = DrawingArea::new();

            underline.set_size_request(32, 4);

            underline.set_halign(gtk4::Align::Center);

            underline_box.append(&underline);

            underline_areas.push(underline);
        }

        // 右侧：内存进度条

        let memory_progress = ProgressBar::new();

        memory_progress.set_size_request(200, 3);

        memory_progress.set_halign(gtk4::Align::End);

        memory_progress.set_valign(gtk4::Align::Start);

        // 底部行的弹性空间

        let bottom_spacer = Box::new(Orientation::Horizontal, 0);

        bottom_spacer.set_hexpand(true);

        // 组装底部行

        bottom_hbox.append(&underline_box);

        bottom_hbox.append(&bottom_spacer);

        bottom_hbox.append(&memory_progress);

        // 组装主容器

        main_vbox.append(&top_hbox);

        main_vbox.append(&bottom_hbox);

        window.set_child(Some(&main_vbox));

        // 应用 CSS 样式
        Self::apply_styles();

        let app_instance = Rc::new(Self {
            window,
            tab_buttons,
            layout_label,
            time_label,
            monitor_label,
            memory_progress,
            cpu_drawing_area,
            underline_areas,
            state,
        });

        // 设置事件处理器
        Self::setup_event_handlers(
            app_instance.clone(),
            layout_button_1,
            layout_button_2,
            layout_button_3,
            screenshot_button,
        );

        app_instance
    }

    fn apply_styles() {
        let provider = gtk4::CssProvider::new();
        provider.load_from_string(
            r#"
        window {
            background-color: rgba(1,1,1,0.8);
        }
        /* 标签按钮基础样式 */
        .tab-button {
            border-radius: 4px;
            margin: 0px;
            padding: 4px 8px;
            font-size: 18px;
            border: 1px solid rgba(255,255,255,0.3);
            background-color: rgba(0,0,0,0.1);
            color: white;
        }
        /* 选中状态 */
        .tab-button.selected {
            background-color: #4ECDC4;
            color: white;
            font-weight: bold;
            border: 2px solid #4ECDC4;
        }
        /* 占用状态（有窗口但未选中） */
        .tab-button.occupied {
            background-color: rgba(255,255,255,0.3);
            border: 1px solid #FECA57;
            color: #FECA57;
        }
        /* 选中且占用状态 */
        .tab-button.selected.occupied {
            background-color: #4ECDC4;
            border: 2px solid #FECA57;
            color: white;
            font-weight: bold;
        }
        /* 填满状态 */
        .tab-button.filled {
            background-color: rgba(0,255,0,0.4);
            border: 2px solid #00FF00;
            color: #00FF00;
            font-weight: bold;
        }
        /* 紧急状态 */
        .tab-button.urgent {
            background-color: rgba(255,0,0,0.6);
            border: 2px solid #FF0000;
            color: white;
            font-weight: bold;
            animation: urgent-blink 1s ease-in-out infinite alternate;
        }
        /* 空闲状态（无窗口且未选中） */
        .tab-button.empty {
            background-color: rgba(102,102,102,0.3);
            border: 1px solid rgba(255,255,255,0.2);
            color: rgba(255,255,255,0.6);
        }
        /* 紧急状态闪烁动画 */
        @keyframes urgent-blink {
            0% { background-color: rgba(255,0,0,0.6); }
            100% { background-color: rgba(255,0,0,0.9); }
        }
        /* 下划线样式 */
        .underline-selected {
            background-color: #4ECDC4;
        }
        .underline-occupied {
            background-color: #FECA57;
        }
        .underline-filled {
            background-color: #00FF00;
        }
        .underline-urgent {
            background-color: #FF0000;
        }
        .underline-empty {
            background-color: transparent;
        }
        /* 其他现有样式 */
        .time-button {
            border-radius: 2px;
            border: 1px solid white;
            padding: 2px 4px;
            background-color: rgba(0,0,0,0.1);
        }
        .time-button:hover {
            background-color: cyan;
            color: darkorange;
        }
        .layout-button {
            font-size: 12px;
            padding: 2px 4px;
        }
        .screenshot-button {
            border-radius: 2px;
            border: 0.5px solid white;
            padding: 0px;
            background-color: rgba(0,0,0,0.1);
        }
        .screenshot-button:hover {
            background-color: cyan;
            color: darkorange;
        }
        "#,
        );

        gtk4::style_context_add_provider_for_display(
            &gtk4::gdk::Display::default().unwrap(),
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    fn setup_event_handlers(
        app: Rc<Self>,
        layout_button_1: Button,
        layout_button_2: Button,
        layout_button_3: Button,
        screenshot_button: Button,
    ) {
        // 设置定时器进行定期更新
        timeout_add_local(Duration::from_millis(50), {
            let app = app.clone();
            move || {
                Self::handle_tick(app.clone());
                glib::ControlFlow::Continue
            }
        });

        timeout_add_local(Duration::from_secs(1), {
            let app = app.clone();
            move || {
                Self::handle_update_time(app.clone());
                glib::ControlFlow::Continue
            }
        });

        // 设置标签按钮点击事件
        for (i, button) in app.tab_buttons.iter().enumerate() {
            button.add_css_class("tab-button");
            button.connect_clicked({
                let app = app.clone();
                move |_| {
                    Self::handle_tab_selected(app.clone(), i);
                }
            });
        }

        // 布局按钮事件
        layout_button_1.connect_clicked({
            let app = app.clone();
            move |_| {
                Self::handle_layout_clicked(app.clone(), 0);
            }
        });

        layout_button_2.connect_clicked({
            let app = app.clone();
            move |_| {
                Self::handle_layout_clicked(app.clone(), 1);
            }
        });

        layout_button_3.connect_clicked({
            let app = app.clone();
            move |_| {
                Self::handle_layout_clicked(app.clone(), 2);
            }
        });

        // 时间按钮点击事件
        app.time_label.add_css_class("time-button");
        app.time_label.connect_clicked({
            let app = app.clone();
            move |_| {
                Self::handle_toggle_seconds(app.clone());
            }
        });

        // 截图按钮事件
        screenshot_button.add_css_class("screenshot-button");
        screenshot_button.connect_clicked({
            let app = app.clone();
            move |_| {
                Self::handle_screenshot(app.clone());
            }
        });

        // CPU 绘制
        app.cpu_drawing_area.set_draw_func({
            let app = app.clone();
            move |_, ctx, width, height| {
                Self::draw_cpu_usage(app.clone(), ctx, width, height);
            }
        });

        // 鼠标事件
        // let motion_controller = EventControllerMotion::new();
        // motion_controller.connect_enter({
        //     let app = app.clone();
        //     move |_, _, _| {
        //         if let Ok(mut state) = app.state.lock() {
        //         }
        //     }
        // });
        // motion_controller.connect_leave({
        //     let app = app.clone();
        //     move |_| {
        //         if let Ok(mut state) = app.state.lock() {
        //         }
        //     }
        // });
        // screenshot_button.add_controller(motion_controller);
    }

    // 事件处理方法
    fn handle_tab_selected(app: Rc<Self>, index: usize) {
        info!("Tab selected: {}", index);

        if let Ok(mut state) = app.state.lock() {
            state.active_tab = index;

            // 发送命令到共享内存
            if let Some(ref command_sender) = state.command_sender {
                Self::send_tag_command(&state, command_sender, true);
            }
        }

        app.update_tab_styles();
    }

    fn handle_layout_clicked(app: Rc<Self>, layout_index: u32) {
        if let Ok(state) = app.state.lock() {
            if let Some(ref message) = state.last_shared_message {
                let monitor_id = message.monitor_info.monitor_num;
                let command = SharedCommand::new(CommandType::SetLayout, layout_index, monitor_id);
                if let Some(ref command_sender) = state.command_sender {
                    if let Err(e) = command_sender.send(command) {
                        error!("Failed to send SetLayout command: {}", e);
                    } else {
                        info!("Sent SetLayout command: layout_index={}", layout_index);
                    }
                }
            }
        }
    }

    fn handle_update_time(app: Rc<Self>) {
        app.update_time_display();
    }

    fn handle_toggle_seconds(app: Rc<Self>) {
        if let Ok(mut state) = app.state.lock() {
            state.show_seconds = !state.show_seconds;
        }
        app.update_time_display();
    }

    fn handle_screenshot(_app: Rc<Self>) {
        info!("Taking screenshot");
        std::process::Command::new("flameshot")
            .arg("gui")
            .spawn()
            .ok();
    }

    fn handle_tick(app: Rc<Self>) {
        // 更新系统监控
        if let Ok(mut state) = app.state.lock() {
            state.system_monitor.update_if_needed();
            state.audio_manager.update_if_needed();
        }

        // 更新UI
        app.update_memory_progress();
        app.cpu_drawing_area.queue_draw();

        // 处理待处理的消息
        app.process_pending_messages();
    }

    // 根据标签状态更新样式
    fn update_tab_styles(&self) {
        if let Ok(state) = self.state.lock() {
            for (i, button) in self.tab_buttons.iter().enumerate() {
                // 先清除所有样式类
                button.remove_css_class("selected");
                button.remove_css_class("occupied");
                button.remove_css_class("filled");
                button.remove_css_class("urgent");
                button.remove_css_class("empty");

                // 获取对应的标签状态
                if let Some(tag_status) = state.tag_status_vec.get(i) {
                    // 根据优先级应用样式
                    if tag_status.is_urg {
                        button.add_css_class("urgent");
                    } else if tag_status.is_filled {
                        button.add_css_class("filled");
                    } else if tag_status.is_selected && tag_status.is_occ {
                        button.add_css_class("selected");
                        button.add_css_class("occupied");
                    } else if tag_status.is_selected && !tag_status.is_occ {
                        button.add_css_class("selected");
                    } else if !tag_status.is_selected && tag_status.is_occ {
                        button.add_css_class("occupied");
                    } else {
                        button.add_css_class("empty");
                    }
                } else {
                    // 回退到简单的活动状态检查
                    if i == state.active_tab {
                        button.add_css_class("selected");
                    } else {
                        button.add_css_class("empty");
                    }
                }
            }
        }

        // 同时更新下划线
        self.update_underlines();
    }

    // 更新下划线显示
    fn update_underlines(&self) {
        if let Ok(state) = self.state.lock() {
            for (i, underline) in self.underline_areas.iter().enumerate() {
                // 设置绘制函数
                underline.set_draw_func({
                    let tag_status = state.tag_status_vec.get(i).cloned();
                    move |_, ctx, width, height| {
                        Self::draw_underline(ctx, width, height, &tag_status);
                    }
                });
                // 触发重绘
                underline.queue_draw();
            }
        }
    }

    // 绘制下划线的静态方法
    fn draw_underline(ctx: &Context, width: i32, height: i32, tag_status: &Option<TagStatus>) {
        let width_f = width as f64;
        let height_f = height as f64;

        // 清除背景
        ctx.set_source_rgba(0.0, 0.0, 0.0, 0.0);
        ctx.paint().ok();

        if let Some(status) = tag_status {
            let (color, line_height) = if status.is_urg {
                // 紧急状态：红色，高4px
                ((1.0, 0.0, 0.0, 0.9), 4.0)
            } else if status.is_filled {
                // 填满状态：绿色，高4px
                ((0.0, 1.0, 0.0, 0.9), 4.0)
            } else if status.is_selected && status.is_occ {
                // 选中且占用：青色，高3px
                ((0.31, 0.80, 0.77, 0.9), 3.0)
            } else if status.is_selected && !status.is_occ {
                // 仅选中：灰色，高3px
                ((0.4, 0.4, 0.4, 0.8), 3.0)
            } else if !status.is_selected && status.is_occ {
                // 仅占用：黄色，高1px
                ((1.0, 0.79, 0.34, 0.8), 1.0)
            } else {
                // 空闲状态：不绘制
                return;
            };

            // 居中绘制长28px的线条
            let line_width = 28.0;
            let x_offset = (width_f - line_width) / 2.0;
            let y_offset = height_f - line_height;

            ctx.set_source_rgba(color.0, color.1, color.2, color.3);
            ctx.rectangle(x_offset, y_offset, line_width, line_height);
            ctx.fill().ok();
        }
    }

    fn update_time_display(&self) {
        let now = Local::now();
        let show_seconds = if let Ok(state) = self.state.lock() {
            state.show_seconds
        } else {
            false
        };

        let format_str = if show_seconds {
            "%Y-%m-%d %H:%M:%S"
        } else {
            "%Y-%m-%d %H:%M"
        };
        let formatted_time = now.format(format_str).to_string();
        self.time_label.set_label(&formatted_time);
    }

    fn update_memory_progress(&self) {
        if let Ok(state) = self.state.lock() {
            if let Some(snapshot) = state.system_monitor.get_snapshot() {
                let total = snapshot.memory_available + snapshot.memory_used;
                let usage_ratio = snapshot.memory_used as f64 / total as f64;
                self.memory_progress.set_fraction(usage_ratio);
            }
        }
    }

    fn monitor_num_to_icon(monitor_num: u8) -> &'static str {
        match monitor_num {
            0 => "🥇",
            1 => "🥈",
            2 => "🥉",
            _ => "?",
        }
    }

    fn send_tag_command(
        state: &AppState,
        command_sender: &mpsc::Sender<SharedCommand>,
        is_view: bool,
    ) {
        if let Some(ref message) = state.last_shared_message {
            let monitor_id = message.monitor_info.monitor_num;
            let tag_bit = 1 << state.active_tab;
            let command = if is_view {
                SharedCommand::view_tag(tag_bit, monitor_id)
            } else {
                SharedCommand::toggle_tag(tag_bit, monitor_id)
            };

            match command_sender.send(command) {
                Ok(_) => {
                    let action = if is_view { "ViewTag" } else { "ToggleTag" };
                    info!(
                        "Sent {} command for tag {} in channel",
                        action,
                        state.active_tab + 1
                    );
                }
                Err(e) => {
                    let action = if is_view { "ViewTag" } else { "ToggleTag" };
                    error!("Failed to send {} command: {}", action, e);
                }
            }
        }
    }

    fn process_pending_messages(&self) {
        let mut need_update = false;
        let mut need_resize = false;
        let mut new_x = 0;
        let mut new_y = 0;
        let mut new_width = 0;
        let mut new_height = 0;

        if let Ok(mut state) = self.state.lock() {
            let messages = state.pending_messages.drain(..).collect::<Vec<_>>();
            if !messages.is_empty() {
                need_update = true;
            }

            for message in &messages {
                info!("Processing shared message: {:?}", message);

                // 获取当前窗口大小
                let current_width = self.window.width();
                let current_height = self.window.height();
                let monitor_width = message.monitor_info.monitor_width;
                let monitor_height = message.monitor_info.monitor_height;
                let border_width = message.monitor_info.border_w;

                info!(
                    "Current window size: {}x{}, Monitor size: {}x{}",
                    current_width, current_height, monitor_width, monitor_height
                );

                // 计算状态栏应该的大小（通常宽度等于监视器宽度，高度固定）
                let expected_x = border_width;
                let expected_y = border_width / 2;
                let expected_width = monitor_width - 2 * border_width;
                let expected_height = 45; // 或者根据需要调整

                // 检查是否需要调整窗口大小（允许小的误差）
                let width_diff = (current_width - expected_width).abs();
                let height_diff = (current_height - expected_height).abs();

                if width_diff > 1 || height_diff > 1 {
                    // 允许5像素的误差
                    need_resize = true;
                    new_x = expected_x;
                    new_y = expected_y;
                    new_width = expected_width;
                    new_height = expected_height;

                    info!(
                        "Window resize needed: current({}x{}) -> expected({}x{}), diff({},{})",
                        current_width,
                        current_height,
                        expected_width,
                        expected_height,
                        width_diff,
                        height_diff
                    );
                } else {
                    info!("Window size is appropriate, no resize needed");
                }

                state.last_shared_message = Some(message.clone());
                state.layout_symbol = message.monitor_info.ltsymbol.clone();
                state.monitor_num = message.monitor_info.monitor_num as u8;

                // 更新标签状态向量
                state.tag_status_vec = message.monitor_info.tag_status_vec.clone();

                // 更新活动标签
                for (index, tag_status) in message.monitor_info.tag_status_vec.iter().enumerate() {
                    if tag_status.is_selected {
                        state.active_tab = index;
                    }
                }
            }
        }

        // 先调整窗口大小，再更新UI
        if need_resize {
            self.resize_window_to_monitor(new_x, new_y, new_width, new_height);
        }

        if need_update {
            self.update_ui();
        }
    }

    fn resize_window_to_monitor(
        &self,
        expected_x: i32,
        expected_y: i32,
        expected_width: i32,
        expected_height: i32,
    ) {
        let current_width = self.window.width();
        let current_height = self.window.height();
        info!(
            "Resizing window: {}x{} -> {}x{}",
            current_width, current_height, expected_width, expected_height
        );
        // 设置新的默认大小
        self.window
            .set_default_size(expected_width, expected_height);
        let display = gtk4::gdk::Display::default().unwrap();
        unsafe {
            if let Some(x11_display) = display.downcast_ref::<gdk4_x11::X11Display>() {
                // 获取 X Display
                let xdisplay = x11_display.xdisplay();
                // 获取窗口 surface
                let surface = self.window.surface().unwrap();
                // 转换为 X11 surface
                if let Some(x11_surface) = surface.downcast_ref::<gdk4_x11::X11Surface>() {
                    let xwindow = x11_surface.xid();
                    XMoveWindow(xdisplay as *mut _, xwindow, expected_x, expected_y);
                    XFlush(xdisplay as *mut _);
                }
            }
        }
    }

    fn update_ui(&self) {
        if let Ok(state) = self.state.lock() {
            self.layout_label.set_text(&state.layout_symbol);

            let monitor_icon = Self::monitor_num_to_icon(state.monitor_num);
            self.monitor_label.set_text(monitor_icon);
        }

        // 重要：更新标签样式（包括下划线）
        self.update_tab_styles();
    }

    fn draw_cpu_usage(app: Rc<Self>, ctx: &Context, width: i32, height: i32) {
        let cpu_usage = if let Ok(state) = app.state.lock() {
            if let Some(snapshot) = state.system_monitor.get_snapshot() {
                snapshot.cpu_average as f64 / 100.0
            } else {
                0.0
            }
        } else {
            0.0
        };

        let width_f = width as f64;
        let height_f = height as f64;

        // 清除背景
        ctx.set_source_rgba(0.0, 0.0, 0.0, 0.0);
        ctx.paint().ok();

        // 绘制背景
        ctx.set_source_rgba(0.0, 0.0, 0.0, 0.3);
        ctx.rectangle(0.0, 0.0, width_f, height_f);
        ctx.fill().ok();

        // 绘制 CPU 使用率条
        let used_height = height_f * cpu_usage;
        let y_offset = height_f - used_height;

        // 设置渐变色
        let gradient = cairo::LinearGradient::new(0.0, 0.0, 0.0, height_f);
        gradient.add_color_stop_rgba(1.0, 1.0, 0.0, 0.0, 0.8); // 红色
        gradient.add_color_stop_rgba(0.0, 0.0, 1.0, 1.0, 0.8); // 青色

        ctx.set_source(&gradient).ok();
        ctx.rectangle(0.0, y_offset, width_f, used_height);
        ctx.fill().ok();
    }

    fn with_channels(&self, command_sender: mpsc::Sender<SharedCommand>) {
        if let Ok(mut state) = self.state.lock() {
            state.command_sender = Some(command_sender);
        }
    }

    fn show(&self) {
        self.window.present();
    }
}

fn shared_memory_worker(
    shared_path: String,
    app_state: SharedAppState,
    command_receiver: mpsc::Receiver<SharedCommand>,
) {
    info!("Starting shared memory worker thread");
    let shared_buffer_opt: Option<SharedRingBuffer> = if shared_path.is_empty() {
        warn!("No shared path provided, running without shared memory");
        None
    } else {
        match SharedRingBuffer::open(&shared_path) {
            Ok(shared_buffer) => {
                info!("Successfully opened shared ring buffer: {}", shared_path);
                Some(shared_buffer)
            }
            Err(e) => {
                warn!(
                    "Failed to open shared ring buffer: {}, attempting to create new one",
                    e
                );
                match SharedRingBuffer::create(&shared_path, None, None) {
                    Ok(shared_buffer) => {
                        info!("Created new shared ring buffer: {}", shared_path);
                        Some(shared_buffer)
                    }
                    Err(create_err) => {
                        error!("Failed to create shared ring buffer: {}", create_err);
                        None
                    }
                }
            }
        }
    };

    let mut prev_timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();

    loop {
        // 处理发送到共享内存的命令
        while let Ok(cmd) = command_receiver.try_recv() {
            info!("Receive command: {:?} in channel", cmd);
            if let Some(ref shared_buffer) = shared_buffer_opt {
                match shared_buffer.send_command(cmd) {
                    Ok(true) => {
                        info!("Sent command: {:?} by shared_buffer", cmd);
                    }
                    Ok(false) => {
                        warn!("Command buffer full, command dropped");
                    }
                    Err(e) => {
                        error!("Failed to send command: {}", e);
                    }
                }
            }
        }

        // 处理共享内存消息
        if let Some(ref shared_buffer) = shared_buffer_opt {
            match shared_buffer.try_read_latest_message::<SharedMessage>() {
                Ok(Some(message)) => {
                    if prev_timestamp != message.timestamp {
                        prev_timestamp = message.timestamp;

                        // 将消息添加到共享状态中
                        if let Ok(mut state) = app_state.lock() {
                            state.pending_messages.push(message);
                        }
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    error!("Ring buffer read error: {}", e);
                }
            }
        }

        thread::sleep(Duration::from_millis(10));
    }
}

/// Initialize logging system
fn initialize_logging(shared_path: &str) -> Result<(), AppError> {
    let now = Local::now();
    let timestamp = now.format("%Y-%m-%d_%H_%M_%S").to_string();

    let file_name = if shared_path.is_empty() {
        STATUS_BAR_PREFIX.to_string()
    } else {
        std::path::Path::new(shared_path)
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| format!("{}_{}", STATUS_BAR_PREFIX, name))
            .unwrap_or_else(|| STATUS_BAR_PREFIX.to_string())
    };

    let log_filename = format!("{}_{}", file_name, timestamp);
    info!("log_filename: {}", log_filename);

    Logger::try_with_str("info")
        .map_err(|e| AppError::config(format!("Failed to create logger: {}", e)))?
        .format(flexi_logger::colored_opt_format)
        .log_to_file(
            FileSpec::default()
                .directory("/tmp")
                .basename(log_filename)
                .suffix("log"),
        )
        .duplicate_to_stdout(Duplicate::Debug)
        .rotate(
            Criterion::Size(10_000_000), // 10MB
            Naming::Numbers,
            Cleanup::KeepLogFiles(5),
        )
        .start()
        .map_err(|e| AppError::config(format!("Failed to start logger: {}", e)))?;

    Ok(())
}

fn main() -> glib::ExitCode {
    let args: Vec<String> = env::args().collect();
    let shared_path = args.get(1).cloned().unwrap_or_default();

    if let Err(e) = initialize_logging(&shared_path) {
        error!("Failed to initialize logging: {}", e);
        std::process::exit(1);
    }

    let instance_name = shared_path.replace("/dev/shm/monitor_", "gtk_bar_");
    info!("instance_name: {instance_name}");
    info!("Starting GTK4 Bar v1.0");

    // 创建 GTK 应用 - 修复版本
    let app = Application::builder()
        .application_id(&format!("{}.{}", instance_name, instance_name))
        .flags(gio::ApplicationFlags::HANDLES_OPEN | gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();

    let shared_path_clone = shared_path.clone();
    app.connect_activate(move |app| {
        // 创建通信通道
        let (_message_sender, _message_receiver) = mpsc::channel::<SharedMessage>();
        let (command_sender, command_receiver) = mpsc::channel::<SharedCommand>();

        // 创建应用实例
        let app_instance = TabBarApp::new(app);

        // 设置命令发送器
        app_instance.with_channels(command_sender);

        // 启动共享内存工作线程
        let app_state = app_instance.state.clone();
        let shared_path_for_thread = shared_path_clone.clone();
        thread::spawn(move || {
            shared_memory_worker(shared_path_for_thread, app_state, command_receiver);
        });

        // 显示窗口
        app_instance.show();
    });

    // 添加文件打开处理
    app.connect_open(move |app, files, hint| {
        info!(
            "App received {} files to open with hint: {}",
            files.len(),
            hint
        );
        for file in files {
            if let Some(path) = file.path() {
                info!("File to open: {:?}", path);
                // 这里可以根据需要处理特定文件
            }
        }
        // 激活主应用
        app.activate();
    });

    // 添加命令行处理
    app.connect_command_line(move |app, command_line| {
        let args = command_line.arguments();
        info!("Command line arguments: {:?}", args);
        app.activate();
        0 // 返回0表示成功
    });

    app.run()
}
