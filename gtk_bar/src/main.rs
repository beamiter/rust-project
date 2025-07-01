use cairo::Context;
use chrono::Local;
use flexi_logger::{Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming};
use glib::timeout_add_local;
use gtk4::gio::{self};
use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box, Button, DrawingArea, EventControllerMotion, Label,
    Orientation, ProgressBar, ScrolledWindow, glib,
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
use shared_structures::{CommandType, SharedCommand, SharedMessage, SharedRingBuffer};
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
    is_hovered: bool,

    // System components
    audio_manager: AudioManager,
    system_monitor: SystemMonitor,

    // Communication
    command_sender: Option<mpsc::Sender<SharedCommand>>,
    last_shared_message: Option<SharedMessage>,

    // 新增：用于在线程间传递消息的队列
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
            is_hovered: false,
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
            .default_width(800)
            .default_height(40)
            .decorated(false)
            .resizable(true)
            .build();

        // 创建主容器
        let main_box = Box::new(Orientation::Vertical, 2);
        main_box.set_margin_top(2);
        main_box.set_margin_bottom(2);
        main_box.set_margin_start(2);
        main_box.set_margin_end(2);

        // 创建工作区行
        let workspace_box = Box::new(Orientation::Horizontal, 3);
        workspace_box.set_valign(gtk4::Align::Center);

        // 创建标签按钮
        let mut tab_buttons = Vec::new();
        let tab_box = Box::new(Orientation::Horizontal, 1);

        for (_, tab_text) in tabs.iter().enumerate() {
            let button = Button::builder()
                .label(tab_text)
                .width_request(32)
                .height_request(32)
                .build();

            tab_box.append(&button);
            tab_buttons.push(button);
        }

        // 布局标签
        let layout_label = Label::new(Some(" ? "));
        layout_label.set_halign(gtk4::Align::Center);

        // 布局按钮区域
        let layout_box = Box::new(Orientation::Horizontal, 10);
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
        layout_scroll.set_size_request(50, 32);
        layout_scroll.set_child(Some(&layout_box));

        // CPU 绘制区域
        let cpu_drawing_area = DrawingArea::new();
        cpu_drawing_area.set_size_request(32, 32);

        // 截图按钮
        let screenshot_button = Button::with_label(&format!(" s {:.2} ", 1.0));

        // 时间按钮
        let time_label = Button::with_label("--:--");

        // 显示器标签
        let monitor_label = Label::new(Some("🥇"));

        // 组装工作区行
        workspace_box.append(&tab_box);
        workspace_box.append(&layout_label);
        workspace_box.append(&layout_scroll);

        // 添加弹性空间
        let spacer = Box::new(Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        workspace_box.append(&spacer);

        workspace_box.append(&cpu_drawing_area);
        workspace_box.append(&screenshot_button);
        workspace_box.append(&time_label);
        workspace_box.append(&monitor_label);

        // 创建下划线行
        let underline_box = Box::new(Orientation::Horizontal, 1);
        underline_box.set_valign(gtk4::Align::Start);
        underline_box.set_height_request(5);

        // 添加标签下划线
        for i in 0..tabs.len() {
            let underline = DrawingArea::new();
            underline.set_size_request(32, 3);
            underline_box.append(&underline);

            if i < tabs.len() - 1 {
                let spacer = Box::new(Orientation::Horizontal, 0);
                spacer.set_size_request(1, 3);
                underline_box.append(&spacer);
            }
        }

        // 内存进度条
        let memory_progress = ProgressBar::new();
        memory_progress.set_size_request(200, 3);
        memory_progress.set_hexpand(false);
        memory_progress.set_halign(gtk4::Align::End);

        let underline_spacer = Box::new(Orientation::Horizontal, 0);
        underline_spacer.set_hexpand(true);
        underline_box.append(&underline_spacer);
        underline_box.append(&memory_progress);

        // 组装主容器
        main_box.append(&workspace_box);
        main_box.append(&underline_box);
        window.set_child(Some(&main_box));

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
                background-color: transparent;
            }
            .tab-button {
                border-radius: 4px;
                margin: 0px;
                padding: 4px 8px;
                font-size: 18px;
                border: 1px solid rgba(255,255,255,0.3);
            }
            .tab-button.active {
                background-color: #4ECDC4;
                color: white;
                font-weight: bold;
            }
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
        let motion_controller = EventControllerMotion::new();

        motion_controller.connect_enter({
            let app = app.clone();
            move |_, _, _| {
                if let Ok(mut state) = app.state.lock() {
                    state.is_hovered = true;
                }
            }
        });

        motion_controller.connect_leave({
            let app = app.clone();
            move |_| {
                if let Ok(mut state) = app.state.lock() {
                    state.is_hovered = false;
                }
            }
        });

        screenshot_button.add_controller(motion_controller);
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
        // 检查共享内存消息
        app.check_shared_messages();

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

    fn update_tab_styles(&self) {
        if let Ok(state) = self.state.lock() {
            for (i, button) in self.tab_buttons.iter().enumerate() {
                if i == state.active_tab {
                    button.add_css_class("active");
                } else {
                    button.remove_css_class("active");
                }
            }
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

    fn update_ui(&self) {
        if let Ok(state) = self.state.lock() {
            self.layout_label.set_text(&state.layout_symbol);

            let monitor_icon = Self::monitor_num_to_icon(state.monitor_num);
            self.monitor_label.set_text(monitor_icon);
        }

        self.update_tab_styles();
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

    fn check_shared_messages(&self) {
        // 这里需要一个共享的消息接收器
        // 由于没有直接的方式在这里访问，我们将在外部处理
    }

    fn process_pending_messages(&self) {
        if let Ok(mut state) = self.state.lock() {
            let messages = state.pending_messages.drain(..).collect::<Vec<_>>();
            for message in messages {
                info!("Processing shared message: {:?}", message);
                state.last_shared_message = Some(message.clone());
                state.layout_symbol = message.monitor_info.ltsymbol.clone();
                state.monitor_num = message.monitor_info.monitor_num as u8;

                // 更新活动标签
                for (index, tag_status) in message.monitor_info.tag_status_vec.iter().enumerate() {
                    if tag_status.is_selected {
                        state.active_tab = index;
                    }
                }
            }
        }

        if !self.state.lock().unwrap().pending_messages.is_empty() {
            self.update_ui();
        }
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
        gradient.add_color_stop_rgba(0.0, 0.0, 1.0, 1.0, 0.8); // 青色
        gradient.add_color_stop_rgba(1.0, 1.0, 0.0, 0.0, 0.8); // 红色

        ctx.set_source(&gradient).ok();
        ctx.rectangle(0.0, y_offset, width_f, used_height);
        ctx.fill().ok();
    }

    fn with_channels(&self, command_sender: mpsc::Sender<SharedCommand>) {
        if let Ok(mut state) = self.state.lock() {
            state.command_sender = Some(command_sender);
        }
    }

    #[allow(dead_code)]
    fn add_shared_message(&self, message: SharedMessage) {
        if let Ok(mut state) = self.state.lock() {
            state.pending_messages.push(message);
        }
    }

    fn show(&self) {
        self.window.present();
    }
}

// 共享内存工作线程 - 修改为使用不同的komunikation方式
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
                Ok(None) => {}
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
