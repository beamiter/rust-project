use chrono::Local;
use gdk4::prelude::*;
#[cfg(all(target_os = "linux", feature = "x11"))]
use gdk4_x11::x11::xlib::{XFlush, XMoveWindow};
use gtk4::gio::{self};
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, Builder, Button, Label, Revealer, glib};
use log::{error, info, warn};
use std::cell::{Cell, RefCell};
use std::env;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender as StdSender};
use std::thread;
use std::time::Duration;

use shared_structures::{CommandType, SharedCommand, SharedMessage, SharedRingBuffer, TagStatus};
use xbar_core::audio_manager::AudioManager;
use xbar_core::initialize_logging;
use xbar_core::system_monitor::SystemMonitor;

use gtk4::glib::ControlFlow;

// ========= 事件与命令 =========
enum AppEvent {
    SharedMessage(SharedMessage),
}

enum AppCommand {
    SendCommand(SharedCommand),
    Stop,
}

// ========= 常量 =========
const CPU_REDRAW_THRESHOLD: f64 = 0.01; // 1%
const MEM_REDRAW_THRESHOLD: f64 = 0.005; // 0.5%

// 胶囊颜色阈值（占用比例）
const LEVEL_WARN: f64 = 0.50; // 50%
const LEVEL_HIGH: f64 = 0.75; // 75%
const LEVEL_CRIT: f64 = 0.90; // 90%

// CSS 类 bit 掩码
const CLS_SELECTED: u8 = 1 << 0;
const CLS_OCCUPIED: u8 = 1 << 1;
const CLS_FILLED: u8 = 1 << 2;
const CLS_URGENT: u8 = 1 << 3;
const CLS_EMPTY: u8 = 1 << 4;

// ========= 状态 =========
#[allow(dead_code)]
struct AppState {
    // UI state
    active_tab: usize,
    layout_symbol: String,
    layout_open: bool,
    monitor_num: u8,
    show_seconds: bool,
    tag_status_vec: Vec<TagStatus>,

    // Components
    audio_manager: AudioManager,
    system_monitor: SystemMonitor,

    // Last values to control redraw
    last_cpu_usage: f64,
    last_mem_fraction: f64,

    // 上一帧每个 tab 的 class 掩码，用于差量更新
    last_class_masks: Vec<u8>,

    // 最近消息时间戳
    last_message_ts: u128,
}

impl AppState {
    fn new() -> Self {
        Self {
            active_tab: 0,
            layout_symbol: " ? ".to_string(),
            layout_open: false,
            monitor_num: 0,
            show_seconds: false,
            tag_status_vec: Vec::new(),
            audio_manager: AudioManager::new(),
            system_monitor: SystemMonitor::new(10),
            last_cpu_usage: 0.0,
            last_mem_fraction: 0.0,
            last_class_masks: Vec::new(),
            last_message_ts: 0,
        }
    }
}

type SharedAppState = Rc<RefCell<AppState>>;

// ========= Metric 工具 =========
fn usage_to_level_class(ratio: f64) -> &'static str {
    if ratio >= LEVEL_CRIT {
        "level-crit"
    } else if ratio >= LEVEL_HIGH {
        "level-high"
    } else if ratio >= LEVEL_WARN {
        "level-warn"
    } else {
        "level-ok"
    }
}

// 统一更新“胶囊”标签：文本 + 颜色 class
fn set_metric_capsule(label: &Label, title: &str, ratio: f64) {
    let percent = (ratio * 100.0).round().clamp(0.0, 100.0) as i32;
    label.set_text(&format!("{} {}%", title, percent));

    for cls in ["level-ok", "level-warn", "level-high", "level-crit"] {
        label.remove_css_class(cls);
    }
    label.add_css_class(usage_to_level_class(ratio));
}

// ========= 背景线程句柄 =========
struct WorkerHandle {
    thread: Option<thread::JoinHandle<()>>,
    cmd_tx: StdSender<AppCommand>,
}
impl WorkerHandle {
    fn new(shared_path: String, ui_sender: async_channel::Sender<AppEvent>) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<AppCommand>();
        let handle = thread::spawn(move || {
            worker_thread(shared_path, ui_sender, cmd_rx);
        });
        Self {
            thread: Some(handle),
            cmd_tx,
        }
    }
    fn send_command(&self, cmd: AppCommand) {
        if let Err(e) = self.cmd_tx.send(cmd) {
            warn!("Failed to send command to worker: {}", e);
        }
    }
}
impl Drop for WorkerHandle {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(AppCommand::Stop);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

// ========= 主体应用 =========
struct TabBarApp {
    // GTK widgets
    builder: Builder,
    window: ApplicationWindow,
    tab_buttons: Vec<Button>,
    time_button: Button,
    monitor_label: Label,
    memory_label: Label,
    cpu_label: Label,

    // 新增：布局开关 + 展开选项
    layout_toggle: Button,
    layout_revealer: Revealer,
    layout_btn_tiled: Button,
    layout_btn_floating: Button,
    layout_btn_monocle: Button,

    // Shared state
    state: SharedAppState,

    // Worker
    worker: WorkerHandle,

    // Cached UI-applied values for diff
    ui_last_monitor_num: Cell<u8>,
}

impl TabBarApp {
    fn new(app: &Application, shared_path: String) -> Rc<Self> {
        // 加载 UI
        let builder = Builder::from_string(include_str!("resources/main_layout.ui"));

        // 主窗口
        let window: ApplicationWindow = builder
            .object("main_window")
            .expect("Failed to get main_window from builder");
        window.set_application(Some(app));

        // 标签按钮
        let mut tab_buttons = Vec::new();
        for i in 0..9 {
            let button_id = format!("tab_button_{}", i);
            let button: Button = builder
                .object(&button_id)
                .expect(&format!("Failed to get {} from builder", button_id));
            tab_buttons.push(button);
        }

        // 其他组件
        let time_button: Button = builder
            .object("time_label")
            .expect("Failed to get time_label from builder");
        let monitor_label: Label = builder
            .object("monitor_label")
            .expect("Failed to get monitor_label from builder");
        let memory_label: Label = builder
            .object("memory_label")
            .expect("Failed to get memory_label from builder");
        let cpu_label: Label = builder
            .object("cpu_label")
            .expect("Failed to get cpu_label from builder");

        // 布局开关 + 选项
        let layout_toggle: Button = builder
            .object("layout_toggle")
            .expect("Failed to get layout_toggle");
        let layout_revealer: Revealer = builder
            .object("layout_revealer")
            .expect("Failed to get layout_revealer");
        let layout_btn_tiled: Button = builder
            .object("layout_option_tiled")
            .expect("Failed to get layout_option_tiled");
        let layout_btn_floating: Button = builder
            .object("layout_option_floating")
            .expect("Failed to get layout_option_floating");
        let layout_btn_monocle: Button = builder
            .object("layout_option_monocle")
            .expect("Failed to get layout_option_monocle");

        // 状态
        let state: SharedAppState = Rc::new(RefCell::new(AppState::new()));

        // 样式
        Self::apply_styles();

        // 异步事件通道（worker -> 主线程）
        let (ui_sender, ui_receiver) = async_channel::unbounded::<AppEvent>();

        // Worker
        let worker = WorkerHandle::new(shared_path.clone(), ui_sender);

        let app_instance = Rc::new(Self {
            builder,
            window,
            tab_buttons,
            time_button,
            monitor_label,
            memory_label,
            cpu_label,
            layout_toggle,
            layout_revealer,
            layout_btn_tiled,
            layout_btn_floating,
            layout_btn_monocle,
            state,
            worker,
            ui_last_monitor_num: Cell::new(255),
        });

        // 为 CPU/内存标签添加基础胶囊样式
        app_instance.cpu_label.add_css_class("metric-label");
        app_instance.memory_label.add_css_class("metric-label");

        // 使用 glib::spawn_future_local 在主线程消费异步通道
        {
            let app_clone = app_instance.clone();
            glib::spawn_future_local(async move {
                while let Ok(event) = ui_receiver.recv().await {
                    match event {
                        AppEvent::SharedMessage(message) => {
                            app_clone.on_shared_message(message);
                        }
                    }
                }
            });
        }

        // 事件绑定
        Self::setup_event_handlers(app_instance.clone());

        // 定时器：每秒更新时间
        {
            let app_clone = app_instance.clone();
            glib::timeout_add_seconds_local(1, move || {
                app_clone.update_time_display();
                ControlFlow::Continue
            });
        }
        // 定时器：每2秒更新系统资源（含阈值和等级变化检测）
        {
            let app_clone = app_instance.clone();
            glib::timeout_add_seconds_local(2, move || {
                if let Ok(mut st) = app_clone.state.try_borrow_mut() {
                    st.system_monitor.update_if_needed();
                    if let Some(snapshot_ref) = st.system_monitor.get_snapshot() {
                        let snapshot = snapshot_ref.clone();
                        let total = snapshot.memory_available + snapshot.memory_used;
                        if total > 0 {
                            // 内存占用比例
                            let mem_ratio =
                                (snapshot.memory_used as f64 / total as f64).clamp(0.0, 1.0);
                            let prev_mem = st.last_mem_fraction;
                            let mem_level_changed =
                                usage_to_level_class(mem_ratio) != usage_to_level_class(prev_mem);
                            if (mem_ratio - prev_mem).abs() > MEM_REDRAW_THRESHOLD
                                || mem_level_changed
                            {
                                st.last_mem_fraction = mem_ratio;
                                set_metric_capsule(&app_clone.memory_label, "MEM", mem_ratio);
                            }

                            // CPU 占用比例（0~1）
                            let cpu_ratio = (snapshot.cpu_average as f64 / 100.0).clamp(0.0, 1.0);
                            let prev_cpu = st.last_cpu_usage;
                            let cpu_level_changed =
                                usage_to_level_class(cpu_ratio) != usage_to_level_class(prev_cpu);
                            if (cpu_ratio - prev_cpu).abs() > CPU_REDRAW_THRESHOLD
                                || cpu_level_changed
                            {
                                st.last_cpu_usage = cpu_ratio;
                                set_metric_capsule(&app_clone.cpu_label, "CPU", cpu_ratio);
                            }
                        }
                    }
                }
                ControlFlow::Continue
            });
        }

        // 首次时间显示
        app_instance.update_time_display();
        // 首次布局 UI 同步（默认 closed）
        app_instance.update_layout_ui();

        app_instance
    }

    fn apply_styles() {
        let provider = gtk4::CssProvider::new();
        provider.load_from_data(include_str!("styles.css"));
        if let Some(display) = gtk4::gdk::Display::default() {
            gtk4::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
    }

    fn setup_event_handlers(app: Rc<Self>) {
        // 标签按钮点击
        for (i, button) in app.tab_buttons.iter().enumerate() {
            button.connect_clicked({
                let app = app.clone();
                move |_| {
                    Self::handle_tab_selected(app.clone(), i);
                }
            });
        }

        // 布局开关
        app.layout_toggle.connect_clicked({
            let app = app.clone();
            move |_| {
                if let Ok(mut st) = app.state.try_borrow_mut() {
                    st.layout_open = !st.layout_open;
                }
                app.update_layout_ui();
            }
        });

        // 布局选项
        app.layout_btn_tiled.connect_clicked({
            let app = app.clone();
            move |_| {
                Self::handle_layout_clicked(app.clone(), 0);
            }
        });
        app.layout_btn_floating.connect_clicked({
            let app = app.clone();
            move |_| {
                Self::handle_layout_clicked(app.clone(), 1);
            }
        });
        app.layout_btn_monocle.connect_clicked({
            let app = app.clone();
            move |_| {
                Self::handle_layout_clicked(app.clone(), 2);
            }
        });

        // 时间按钮
        app.time_button.connect_clicked({
            let app = app.clone();
            move |_| {
                Self::handle_toggle_seconds(app.clone());
            }
        });

        // 截图按钮
        if let Some(screenshot_button) = app.builder.object::<Button>("screenshot_button") {
            screenshot_button.connect_clicked({
                let app = app.clone();
                move |_| {
                    Self::handle_screenshot(app.clone());
                }
            });
        }
    }

    // ========= Worker事件处理 =========
    fn on_shared_message(&self, message: SharedMessage) {
        if let Ok(mut st) = self.state.try_borrow_mut() {
            let ts: u128 = message.timestamp.into();
            if st.last_message_ts == ts {
                return; // 去重
            }
            st.last_message_ts = ts;

            st.layout_symbol = message.monitor_info.get_ltsymbol();
            st.monitor_num = message.monitor_info.monitor_num as u8;
            st.tag_status_vec = message.monitor_info.tag_status_vec.to_vec();

            // 更新活动标签
            for (idx, tag) in message.monitor_info.tag_status_vec.iter().enumerate() {
                if tag.is_selected {
                    st.active_tab = idx;
                    break;
                }
            }

            // 确保掩码数组长度匹配
            if st.last_class_masks.len() != self.tab_buttons.len() {
                st.last_class_masks = vec![0u8; self.tab_buttons.len()];
            }
        }
        // 更新 UI（差量）
        self.update_ui();
        self.update_layout_ui();
    }

    // ========= 交互 =========
    fn handle_tab_selected(app: Rc<Self>, index: usize) {
        info!("Tab selected: {}", index);
        if let Ok(mut st) = app.state.try_borrow_mut() {
            st.active_tab = index;
            if let Some(cmd) = Self::build_tag_command(&st, true) {
                app.worker.send_command(AppCommand::SendCommand(cmd));
            }
        }
        app.update_tab_styles();
    }

    fn handle_layout_clicked(app: Rc<Self>, layout_index: u32) {
        if let Ok(st) = app.state.try_borrow() {
            let monitor_id = st.monitor_num as i32;
            let command = SharedCommand::new(CommandType::SetLayout, layout_index, monitor_id);
            app.worker.send_command(AppCommand::SendCommand(command));
            info!("Sent SetLayout command: layout_index={}", layout_index);
        }
        if let Ok(mut st) = app.state.try_borrow_mut() {
            st.layout_open = false; // 选择后收起
        }
        app.update_layout_ui();
    }

    fn handle_toggle_seconds(app: Rc<Self>) {
        if let Ok(mut st) = app.state.try_borrow_mut() {
            st.show_seconds = !st.show_seconds;
        }
        app.update_time_display();
    }

    fn handle_screenshot(_app: Rc<Self>) {
        info!("Taking screenshot");
        let _ = std::process::Command::new("flameshot").arg("gui").spawn();
    }

    // ========= UI 更新 =========
    fn update_ui(&self) {
        if let Ok(st) = self.state.try_borrow() {
            // monitor_label 差量
            if self.ui_last_monitor_num.get() != st.monitor_num {
                let monitor_icon = Self::monitor_num_to_icon(st.monitor_num);
                self.monitor_label.set_text(monitor_icon);
                self.ui_last_monitor_num.set(st.monitor_num);
            }
        }
        self.update_tab_styles();
    }

    fn update_tab_styles(&self) {
        if let Ok(mut st) = self.state.try_borrow_mut() {
            if st.last_class_masks.len() != self.tab_buttons.len() {
                st.last_class_masks = vec![0u8; self.tab_buttons.len()];
            }

            for (i, button) in self.tab_buttons.iter().enumerate() {
                let tag_opt = st.tag_status_vec.get(i);
                let desired_mask = Self::classes_mask_for(tag_opt, i == st.active_tab);
                let prev_mask = st.last_class_masks[i];

                if desired_mask == prev_mask {
                    continue;
                }

                // 移除所有相关 class
                for c in &["selected", "occupied", "filled", "urgent", "empty"] {
                    button.remove_css_class(c);
                }
                // 添加必要 class
                if desired_mask & CLS_URGENT != 0 {
                    button.add_css_class("urgent");
                }
                if desired_mask & CLS_FILLED != 0 {
                    button.add_css_class("filled");
                }
                if desired_mask & CLS_SELECTED != 0 {
                    button.add_css_class("selected");
                }
                if desired_mask & CLS_OCCUPIED != 0 {
                    button.add_css_class("occupied");
                }
                if desired_mask & CLS_EMPTY != 0 {
                    button.add_css_class("empty");
                }

                st.last_class_masks[i] = desired_mask;
            }
        }
    }

    // 新增：布局 UI 更新（切换 open/closed、高亮当前布局、更新 toggle 文本）
    fn update_layout_ui(&self) {
        if let Ok(st) = self.state.try_borrow() {
            // 开关按钮文本：显示当前布局符号
            self.layout_toggle.set_label(&st.layout_symbol);

            // revealer 展开/收起
            self.layout_revealer.set_reveal_child(st.layout_open);

            // 开关按钮 open/closed 类
            self.layout_toggle.remove_css_class("open");
            self.layout_toggle.remove_css_class("closed");
            self.layout_toggle
                .add_css_class(if st.layout_open { "open" } else { "closed" });

            // 当前布局高亮
            let is_tiled = st.layout_symbol.contains("[]=");
            let is_floating = st.layout_symbol.contains("><>");
            let is_monocle = st.layout_symbol.contains("[M]");

            for b in [
                &self.layout_btn_tiled,
                &self.layout_btn_floating,
                &self.layout_btn_monocle,
            ] {
                b.remove_css_class("current");
            }
            if is_tiled {
                self.layout_btn_tiled.add_css_class("current");
            } else if is_floating {
                self.layout_btn_floating.add_css_class("current");
            } else if is_monocle {
                self.layout_btn_monocle.add_css_class("current");
            }
        }
    }

    fn update_time_display(&self) {
        let now = Local::now();
        let show_seconds = if let Ok(st) = self.state.try_borrow() {
            st.show_seconds
        } else {
            false
        };

        let format_str = if show_seconds {
            "%Y-%m-%d %H:%M:%S"
        } else {
            "%Y-%m-%d %H:%M"
        };
        let formatted_time = now.format(format_str).to_string();
        self.time_button.set_label(&formatted_time);
    }

    // ========= 工具 =========
    fn monitor_num_to_icon(monitor_num: u8) -> &'static str {
        match monitor_num {
            0 => "🥇",
            1 => "🥈",
            2 => "🥉",
            _ => "🖥",
        }
    }

    fn classes_mask_for(tag: Option<&TagStatus>, is_active_index: bool) -> u8 {
        if let Some(t) = tag {
            if t.is_urg {
                CLS_URGENT
            } else if t.is_filled {
                CLS_FILLED
            } else if t.is_selected && t.is_occ {
                CLS_SELECTED | CLS_OCCUPIED
            } else if t.is_selected || is_active_index {
                CLS_SELECTED
            } else if t.is_occ {
                CLS_OCCUPIED
            } else {
                CLS_EMPTY
            }
        } else {
            if is_active_index {
                CLS_SELECTED
            } else {
                CLS_EMPTY
            }
        }
    }

    fn build_tag_command(state: &AppState, is_view: bool) -> Option<SharedCommand> {
        if state.active_tab >= 32 {
            return None;
        }
        let tag_bit: u32 = 1u32 << (state.active_tab as u32);
        let monitor_id = state.monitor_num as i32;
        let cmd = if is_view {
            SharedCommand::view_tag(tag_bit, monitor_id)
        } else {
            SharedCommand::toggle_tag(tag_bit, monitor_id)
        };
        Some(cmd)
    }

    #[allow(dead_code)]
    #[cfg(all(target_os = "linux", feature = "x11"))]
    fn resize_window_to_monitor(
        &self,
        expected_x: i32,
        expected_y: i32,
        expected_width: i32,
        expected_height: i32,
    ) {
        self.window
            .set_default_size(expected_width, expected_height);
        if let Some(display) = gtk4::gdk::Display::default() {
            unsafe {
                if let Some(x11_display) = display.downcast_ref::<gdk4_x11::X11Display>() {
                    let xdisplay = x11_display.xdisplay();
                    if let Some(surface) = self.window.surface() {
                        if let Some(x11_surface) = surface.downcast_ref::<gdk4_x11::X11Surface>() {
                            let xwindow = x11_surface.xid();
                            XMoveWindow(xdisplay as *mut _, xwindow, expected_x, expected_y);
                            XFlush(xdisplay as *mut _);
                        }
                    }
                }
            }
        }
    }

    #[allow(dead_code)]
    #[cfg(not(all(target_os = "linux", feature = "x11")))]
    fn resize_window_to_monitor(
        &self,
        _expected_x: i32,
        _expected_y: i32,
        expected_width: i32,
        expected_height: i32,
    ) {
        self.window
            .set_default_size(expected_width, expected_height);
    }

    fn show(&self) {
        self.window.present();
    }
}

// ========= Worker 线程：独占 SharedRingBuffer =========
fn worker_thread(
    shared_path: String,
    ui_sender: async_channel::Sender<AppEvent>,
    cmd_rx: Receiver<AppCommand>,
) {
    info!("Worker thread starting with shared_path={}", shared_path);
    let shared_buffer_opt = SharedRingBuffer::create_shared_ring_buffer_aux(&shared_path);

    if shared_buffer_opt.is_none() {
        error!("Failed to create/open SharedRingBuffer at {}", shared_path);
        return;
    }
    let shared_buffer = shared_buffer_opt.unwrap();
    let mut prev_timestamp: u128 = 0;

    'outer: loop {
        // 处理全部待发送命令（非阻塞）
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                AppCommand::SendCommand(command) => match shared_buffer.send_command(command) {
                    Ok(true) => info!("Command sent via shared_buffer"),
                    Ok(false) => warn!("Command buffer full, command dropped"),
                    Err(e) => error!("Failed to send command: {}", e),
                },
                AppCommand::Stop => {
                    info!("Worker received Stop, exiting");
                    break 'outer;
                }
            }
        }

        // 等待共享内存消息，带超时以便循环处理命令
        match shared_buffer.wait_for_message(Some(Duration::from_millis(500))) {
            Ok(true) => {
                if let Ok(Some(message)) = shared_buffer.try_read_latest_message() {
                    let ts: u128 = message.timestamp.into();
                    if ts != prev_timestamp {
                        prev_timestamp = ts;
                        if let Err(e) = ui_sender.try_send(AppEvent::SharedMessage(message)) {
                            if !e.is_full() {
                                warn!("Failed to send SharedMessage to UI: {}", e);
                            }
                        }
                    }
                }
            }
            Ok(false) => {
                // timeout
            }
            Err(e) => {
                error!("[worker] wait_for_message failed: {}", e);
                thread::sleep(Duration::from_millis(200));
            }
        }
    }

    info!("Worker thread exited");
}

// ========= main =========
fn main() -> glib::ExitCode {
    let args: Vec<String> = env::args().collect();
    let shared_path = args.get(1).cloned().unwrap_or_default();

    if let Err(e) = initialize_logging("gtk_bar", &shared_path) {
        eprintln!("Failed to initialize logging: {}", e);
        std::process::exit(1);
    }

    info!("Starting GTK4 Bar (layout selector optimized like iced_bar)");

    // GTK 应用
    let app = Application::builder()
        .application_id("dev.gtk.bar")
        .flags(gio::ApplicationFlags::HANDLES_OPEN | gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();

    let shared_path_clone = shared_path.clone();
    app.connect_activate(move |app| {
        let app_instance = TabBarApp::new(app, shared_path_clone.clone());
        app_instance.show();

        let app_weak = Rc::downgrade(&app_instance);
        app.connect_shutdown(move |_| {
            let _ = app_weak.upgrade(); // Drop 即触发 worker 停止
        });
    });

    // 文件打开处理
    app.connect_open(move |app, files, hint| {
        info!(
            "App received {} files to open with hint: {}",
            files.len(),
            hint
        );
        for file in files {
            if let Some(path) = file.path() {
                info!("File to open: {:?}", path);
            }
        }
        app.activate();
    });

    // 命令行处理
    app.connect_command_line(move |app, command_line| {
        let args = command_line.arguments();
        info!("Command line arguments: {:?}", args);
        app.activate();
        0.into()
    });

    app.run()
}
