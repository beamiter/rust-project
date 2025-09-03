use async_channel::Sender;
use cairo::Context;
use chrono::Local;
use flexi_logger::{Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming};
use gdk4::prelude::*;
use gdk4_x11::x11::xlib::{XFlush, XMoveWindow};
use gtk4::gio::{self};
use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Builder, Button, DrawingArea, Label, ProgressBar, glib,
};
use log::{error, info, warn};
use std::env;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

mod audio_manager;
mod error;
mod system_monitor;

use audio_manager::AudioManager;
use error::AppError;
use shared_structures::{CommandType, SharedCommand, SharedMessage, SharedRingBuffer, TagStatus};
use system_monitor::SystemMonitor;

use gtk4::glib::ControlFlow;

enum AppEvent {
    SharedMessage(SharedMessage),
    // 如需可扩展其它事件，比如 SystemSnapshot, Audio 等
}

const STATUS_BAR_PREFIX: &str = "gtk_bar";

// 使用 Arc<Mutex<>> 来共享状态
type SharedAppState = Arc<Mutex<AppState>>;

#[allow(dead_code)]
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
    current_message: Option<SharedMessage>,
    shared_buffer_opt: Option<SharedRingBuffer>,

    last_cpu_usage: f64,
    last_mem_fraction: f64,
}

struct TabBarApp {
    // GTK widgets - 从 Builder 获取
    builder: Builder,
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
    fn new(app: &Application, shared_path: String) -> Rc<Self> {
        // 加载 UI 布局
        let builder = Builder::from_string(include_str!("resources/main_layout.ui"));

        // 获取主窗口
        let window: ApplicationWindow = builder
            .object("main_window")
            .expect("Failed to get main_window from builder");
        window.set_application(Some(app));

        // 获取标签按钮
        let mut tab_buttons = Vec::new();
        for i in 0..9 {
            let button_id = format!("tab_button_{}", i);
            let button: Button = builder
                .object(&button_id)
                .expect(&format!("Failed to get {} from builder", button_id));
            tab_buttons.push(button);
        }

        // 获取其他组件
        let layout_label: Label = builder
            .object("layout_label")
            .expect("Failed to get layout_label from builder");

        let time_label: Button = builder
            .object("time_label")
            .expect("Failed to get time_label from builder");

        let monitor_label: Label = builder
            .object("monitor_label")
            .expect("Failed to get monitor_label from builder");

        let memory_progress: ProgressBar = builder
            .object("memory_progress")
            .expect("Failed to get memory_progress from builder");

        let cpu_drawing_area: DrawingArea = builder
            .object("cpu_drawing_area")
            .expect("Failed to get cpu_drawing_area from builder");

        // 创建共享状态
        let shared_buffer_opt = SharedRingBuffer::create_shared_ring_buffer(&shared_path);
        let state = Arc::new(Mutex::new(AppState {
            active_tab: 0,
            layout_symbol: " ? ".to_string(),
            monitor_num: 0,
            show_seconds: false,
            tag_status_vec: Vec::new(),
            audio_manager: AudioManager::new(),
            system_monitor: SystemMonitor::new(10),
            current_message: None,
            shared_buffer_opt,
            last_cpu_usage: 0.0,
            last_mem_fraction: 0.0,
        }));

        // 应用 CSS 样式
        Self::apply_styles();

        let app_instance = Rc::new(Self {
            builder,
            window,
            tab_buttons,
            layout_label,
            time_label,
            monitor_label,
            memory_progress,
            cpu_drawing_area,
            state,
        });

        // ========== 新增：事件通道 ==========
        let (tx, rx) = async_channel::unbounded();
        {
            let app_state_clone = app_instance.state.clone();
            let shared_path_clone = shared_path.clone();
            tokio::spawn(async move {
                shared_memory_worker(shared_path_clone, app_state_clone, tx).await;
            });
        }
        let use_main_context = false;
        // 收到消息则立刻更新 UI（主线程）
        if use_main_context {
            let app_instance_clone = app_instance.clone();
            glib::MainContext::default().spawn_local(async move {
                while let Ok(event) = rx.recv().await {
                    match event {
                        AppEvent::SharedMessage(message) => {
                            if let Ok(mut state) = app_instance_clone.state.lock() {
                                state.current_message = Some(message.clone());
                                state.layout_symbol = message.monitor_info.get_ltsymbol();
                                state.monitor_num = message.monitor_info.monitor_num as u8;
                                state.tag_status_vec = message.monitor_info.tag_status_vec.to_vec();
                                // 更新活动标签
                                for (index, tag_status) in
                                    message.monitor_info.tag_status_vec.iter().enumerate()
                                {
                                    if tag_status.is_selected {
                                        state.active_tab = index;
                                        break;
                                    }
                                }
                            }
                            app_instance_clone.update_ui();
                        }
                    }
                }
            });
        } else {
            glib::spawn_future_local(glib::clone!(
                #[weak]
                app_instance,
                async move {
                    while let Ok(event) = rx.recv().await {
                        match event {
                            AppEvent::SharedMessage(message) => {
                                if let Ok(mut state) = app_instance.state.lock() {
                                    state.current_message = Some(message.clone());
                                    state.layout_symbol = message.monitor_info.get_ltsymbol();
                                    state.monitor_num = message.monitor_info.monitor_num as u8;
                                    state.tag_status_vec =
                                        message.monitor_info.tag_status_vec.to_vec();
                                    // 更新活动标签
                                    for (index, tag_status) in
                                        message.monitor_info.tag_status_vec.iter().enumerate()
                                    {
                                        if tag_status.is_selected {
                                            state.active_tab = index;
                                            break;
                                        }
                                    }
                                }
                                app_instance.update_ui();
                            }
                        }
                    }
                }
            ));
        }

        // 设置事件处理器
        Self::setup_event_handlers(app_instance.clone());

        // 定时器 1: 每秒更新时间（低成本）
        {
            let app_clone = app_instance.clone();
            glib::timeout_add_seconds_local(1, move || {
                app_clone.update_time_display();
                ControlFlow::Continue
            });
        }

        // 定时器 2: 每 2 秒更新系统资源（高成本）
        {
            let app_clone = app_instance.clone();
            glib::timeout_add_seconds_local(2, move || {
                if let Ok(mut state) = app_clone.state.lock() {
                    // 在这里执行昂贵的更新
                    state.system_monitor.update_if_needed();

                    if let Some(snapshot_ref) = state.system_monitor.get_snapshot() {
                        let snapshot = snapshot_ref.clone(); // 正确地解除了借用
                        let total = snapshot.memory_available + snapshot.memory_used;
                        if total > 0 {
                            // ... 内存和 CPU 的 UI 更新逻辑 ...
                            let usage_ratio = snapshot.memory_used as f64 / total as f64;
                            if (usage_ratio - state.last_mem_fraction).abs() > 0.005 {
                                state.last_mem_fraction = usage_ratio;
                                app_clone.memory_progress.set_fraction(usage_ratio);
                            }

                            let cpu_usage = snapshot.cpu_average as f64 / 100.0;
                            if (cpu_usage - state.last_cpu_usage).abs() > 0.01 {
                                state.last_cpu_usage = cpu_usage;
                                app_clone.cpu_drawing_area.queue_draw();
                            }
                        }
                    }
                }
                ControlFlow::Continue
            });
        }

        // 在创建时立即调用一次，避免启动时延迟显示
        app_instance.update_time_display();

        app_instance
    }

    fn apply_styles() {
        let provider = gtk4::CssProvider::new();
        provider.load_from_string(include_str!("styles.css"));
        gtk4::style_context_add_provider_for_display(
            &gtk4::gdk::Display::default().unwrap(),
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    fn setup_event_handlers(app: Rc<Self>) {
        // 设置标签按钮点击事件
        for (i, button) in app.tab_buttons.iter().enumerate() {
            button.connect_clicked({
                let app = app.clone();
                move |_| {
                    Self::handle_tab_selected(app.clone(), i);
                }
            });
        }

        // 布局按钮事件
        for i in 1..=3 {
            let button_id = format!("layout_button_{}", i);
            if let Some(button) = app.builder.object::<Button>(&button_id) {
                button.connect_clicked({
                    let app = app.clone();
                    let layout_index = i - 1; // 转换为0-based索引
                    move |_| {
                        Self::handle_layout_clicked(app.clone(), layout_index);
                    }
                });
            }
        }

        // 时间按钮点击事件
        app.time_label.connect_clicked({
            let app = app.clone();
            move |_| {
                Self::handle_toggle_seconds(app.clone());
            }
        });

        // 截图按钮事件
        if let Some(screenshot_button) = app.builder.object::<Button>("screenshot_button") {
            screenshot_button.connect_clicked({
                let app = app.clone();
                move |_| {
                    Self::handle_screenshot(app.clone());
                }
            });
        }

        // CPU 绘制
        app.cpu_drawing_area.set_draw_func({
            let app = app.clone();
            move |_, ctx, width, height| {
                Self::draw_cpu_usage(app.clone(), ctx, width, height);
            }
        });
    }

    // 事件处理方法保持不变
    fn handle_tab_selected(app: Rc<Self>, index: usize) {
        info!("Tab selected: {}", index);
        if let Ok(mut state) = app.state.lock() {
            state.active_tab = index;
            // 发送命令到共享内存
            Self::send_tag_command(&state, true);
        }
        app.update_tab_styles();
    }

    fn handle_layout_clicked(app: Rc<Self>, layout_index: u32) {
        if let Ok(state) = app.state.lock() {
            if let Some(ref message) = state.current_message {
                let monitor_id = message.monitor_info.monitor_num;
                let command = SharedCommand::new(CommandType::SetLayout, layout_index, monitor_id);
                if let Some(ref shared_buffer) = state.shared_buffer_opt {
                    if let Err(e) = shared_buffer.send_command(command) {
                        error!("Failed to send SetLayout command: {}", e);
                    } else {
                        info!("Sent SetLayout command: layout_index={}", layout_index);
                    }
                }
            }
        }
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

    // UI 更新方法保持不变
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

    // 其他方法保持不变...
    fn monitor_num_to_icon(monitor_num: u8) -> &'static str {
        match monitor_num {
            0 => "🥇",
            1 => "🥈",
            2 => "🥉",
            _ => "?",
        }
    }

    fn send_tag_command(state: &AppState, is_view: bool) {
        if let Some(ref message) = state.current_message {
            let monitor_id = message.monitor_info.monitor_num;
            let tag_bit = 1 << state.active_tab;
            let command = if is_view {
                SharedCommand::view_tag(tag_bit, monitor_id)
            } else {
                SharedCommand::toggle_tag(tag_bit, monitor_id)
            };
            if let Some(shared_buffer) = &state.shared_buffer_opt {
                match shared_buffer.send_command(command) {
                    Ok(true) => {
                        info!("Sent command: {:?} by shared_buffer", command);
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
    }

    #[allow(dead_code)]
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
        // 彩虹渐变
        gradient.add_color_stop_rgba(0.0, 1.0, 0.0, 0.0, 0.9);
        gradient.add_color_stop_rgba(0.5, 1.0, 1.0, 0.0, 0.9);
        gradient.add_color_stop_rgba(1.0, 0.0, 1.0, 1.0, 0.9);

        ctx.set_source(&gradient).ok();
        ctx.rectangle(0.0, y_offset, width_f, used_height);
        ctx.fill().ok();
    }

    fn show(&self) {
        self.window.present();
    }
}

async fn shared_memory_worker(
    shared_path: String,
    app_state: SharedAppState,
    tx: Sender<AppEvent>,
) {
    info!("Starting shared memory worker task");

    let shared_buffer_opt = SharedRingBuffer::create_shared_ring_buffer(&shared_path);
    let mut prev_timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();

    if let Some(ref shared_buffer) = shared_buffer_opt {
        loop {
            match shared_buffer.wait_for_message(Some(std::time::Duration::from_secs(2))) {
                Ok(true) => {
                    if let Ok(Some(message)) = shared_buffer.try_read_latest_message() {
                        if prev_timestamp != message.timestamp.into() {
                            prev_timestamp = message.timestamp.into();
                            // 可选：加一层快速比较，避免重复发送相同消息
                            let need_update = {
                                if let Ok(state) = app_state.lock() {
                                    state
                                        .current_message
                                        .as_ref()
                                        .map(|m| m.timestamp != message.timestamp)
                                        .unwrap_or(true)
                                } else {
                                    true
                                }
                            };
                            if need_update {
                                // 直接通过 channel 通知主线程更新 UI
                                if let Err(e) = tx.send_blocking(AppEvent::SharedMessage(message)) {
                                    warn!("Failed to send AppEvent::SharedMessage: {}", e);
                                }
                            }
                        }
                    }
                }
                Ok(false) => log::debug!("[notifier] Wait for message timed out."),
                Err(e) => {
                    error!("[notifier] Wait for message failed: {}", e);
                    break;
                }
            }
        }
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

#[tokio::main]
async fn main() -> glib::ExitCode {
    let args: Vec<String> = env::args().collect();
    let shared_path = args.get(1).cloned().unwrap_or_default();

    if let Err(e) = initialize_logging(&shared_path) {
        error!("Failed to initialize logging: {}", e);
        std::process::exit(1);
    }

    let mut instance_name = shared_path.replace("/dev/shm/monitor_", "gtk_bar_");
    if instance_name.is_empty() {
        instance_name = "gtk_bar".to_string();
    }
    instance_name = format!("{}.{}", instance_name, instance_name);
    info!("instance_name: {}", instance_name);
    info!("Starting GTK4 Bar v1.0");

    // 创建 GTK 应用
    let app = Application::builder()
        .application_id(instance_name)
        .flags(gio::ApplicationFlags::HANDLES_OPEN | gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();

    let shared_path_clone = shared_path.clone();
    app.connect_activate(move |app| {
        // 创建应用实例
        let shared_path_for_app = shared_path_clone.clone();
        let app_instance = TabBarApp::new(app, shared_path_for_app);

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
