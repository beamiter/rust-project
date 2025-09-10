use chrono::Local;
use flexi_logger::{Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming};
use gdk4_x11::x11::xlib::{XFlush, XMoveWindow};
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use gtk4::Window;
use gtk4::glib::ControlFlow;
use log::{error, info, warn};
use relm4::factory::{DynamicIndex, FactoryComponent, FactorySender, FactoryVecDeque};
use relm4::prelude::*;
use relm4::{ComponentParts, ComponentSender, RelmApp, SimpleComponent};

use std::time::Duration;

mod audio_manager;
mod error;
mod system_monitor;

use error::AppError;
use shared_structures::{CommandType, SharedCommand, SharedMessage, SharedRingBuffer, TagStatus};
use system_monitor::SystemMonitor;

// ========== 子项（Tab） ==========

#[derive(Debug, Clone)]
struct TabInit {
    index: usize,
    emoji: String,
    status: Option<TagStatus>,
}

#[derive(Debug, Clone)]
struct TabItem {
    index: usize,
    emoji: String,
    status: Option<TagStatus>,
}

#[derive(Debug)]
enum TabOutput {
    Clicked(usize),
}

#[relm4::factory]
impl FactoryComponent for TabItem {
    type Init = TabInit;
    type Input = ();
    type Output = TabOutput;
    type CommandOutput = ();
    type ParentWidget = gtk::Box;

    view! {
        gtk::Button {
            set_width_request: 40,
            #[watch]
            set_label: &self.emoji,
            #[watch]
            set_css_classes: &compute_tab_css_classes(self.status.as_ref()),
            // 通过捕获值，避免直接在闭包里用 &self
            connect_clicked[sender, tag_index = self.index] => move |_| {
                let _ = sender.output(TabOutput::Clicked(tag_index));
            }
        }
    }

    fn init_model(init: Self::Init, _index: &DynamicIndex, _sender: FactorySender<Self>) -> Self {
        Self {
            index: init.index,
            emoji: init.emoji,
            status: init.status,
        }
    }

    fn update(&mut self, _msg: Self::Input, _sender: FactorySender<Self>) {}
}

// 返回 &[&str] 兼容的 Vec<&'static str>
fn compute_tab_css_classes(s: Option<&TagStatus>) -> Vec<&'static str> {
    match s {
        Some(st) if st.is_urg => vec!["tab-button", "urgent"],
        Some(st) if st.is_filled => vec!["tab-button", "filled"],
        Some(st) if st.is_selected => vec!["tab-button", "selected"],
        Some(st) if st.is_occ => vec!["tab-button", "occupied"],
        _ => vec!["tab-button", "empty"],
    }
}

// Metric 胶囊的动态等级类
fn metric_css_classes(usage: f64) -> Vec<&'static str> {
    // usage: 0.0 ~ 1.0
    let lvl = if usage < 0.50 {
        "level-ok"
    } else if usage < 0.70 {
        "level-warn"
    } else if usage < 0.85 {
        "level-high"
    } else {
        "level-crit"
    };
    vec!["metric-label", lvl]
}

fn pick_emoji(i: usize) -> &'static str {
    match i {
        0 => "🍜",
        1 => "🎨",
        2 => "🍀",
        3 => "🧿",
        4 => "🌟",
        5 => "🐐",
        6 => "🏆",
        7 => "🕊️",
        8 => "🏡",
        _ => "❔",
    }
}

// ========== 主应用 ==========

#[derive(Debug)]
pub enum AppInput {
    TabSelected(usize),
    LayoutChanged(u32),
    ToggleSeconds,
    Screenshot,
    SharedMessageReceived(SharedMessage),
    SystemUpdate,
    UpdateTime,
}

#[tracker::track]
pub struct AppModel {
    pub active_tab: usize,
    pub layout_symbol: String,
    pub monitor_num: u8,
    pub show_seconds: bool,
    pub tag_status_vec: Vec<TagStatus>,
    pub last_shared_message: Option<SharedMessage>,
    pub memory_usage: f64,
    pub cpu_usage: f64,
    pub current_time: String,

    #[do_not_track]
    shared_buffer_opt: Option<SharedRingBuffer>,
    #[do_not_track]
    pub system_monitor: SystemMonitor,

    // Factory：标签集合
    #[do_not_track]
    tabs: FactoryVecDeque<TabItem>,
}

#[relm4::component(pub)]
impl SimpleComponent for AppModel {
    type Init = String; // 共享路径
    type Input = AppInput;
    type Output = ();

    view! {
        #[root]
        gtk::ApplicationWindow {
            set_decorated: false,
            set_default_size: (1000, 40),
            set_resizable: true,
            add_css_class: "main-window",

            gtk::Box {
                set_orientation: gtk::Orientation::Horizontal,
                set_spacing: 3,
                set_margin_all: 3,

                // 标签栏（Factory容器）
                #[local_ref]
                tabs_box -> gtk::Box {
                    set_orientation: gtk::Orientation::Horizontal,
                    set_spacing: 3,
                },

                // 布局标签
                gtk::Label {
                    #[watch]
                    set_text: &model.layout_symbol,
                    set_width_request: 40,
                    set_halign: gtk::Align::Center,
                    add_css_class: "layout-label",
                },

                // 布局按钮
                gtk::ScrolledWindow {
                    set_hscrollbar_policy: gtk::PolicyType::Automatic,
                    set_vscrollbar_policy: gtk::PolicyType::Never,
                    set_width_request: 120,

                    gtk::Box {
                        set_orientation: gtk::Orientation::Horizontal,
                        set_spacing: 5,

                        gtk::Button {
                            set_label: "[]=",
                            set_height_request: 28,
                            set_width_request: 38,
                            add_css_class: "layout-button",
                            connect_clicked => AppInput::LayoutChanged(0),
                        },

                        gtk::Button {
                            set_label: "<><",
                            set_height_request: 28,
                            set_width_request: 38,
                            add_css_class: "layout-button",
                            connect_clicked => AppInput::LayoutChanged(1),
                        },

                        gtk::Button {
                            set_label: "[M]",
                            set_height_request: 28,
                            set_width_request: 38,
                            add_css_class: "layout-button",
                            connect_clicked => AppInput::LayoutChanged(2),
                        },
                    }
                },

                // 中间间隔
                gtk::Box {
                    set_hexpand: true,
                },

                // 系统信息区域
                gtk::Box {
                    set_halign: gtk::Align::End,
                    set_orientation: gtk::Orientation::Horizontal,
                    set_spacing: 3,

                    // 内存（胶囊标签）
                    gtk::Label {
                        #[watch]
                        set_label: &format!("MEM {:>3}%", (model.memory_usage * 100.0).round() as u32),
                        #[watch]
                        set_css_classes: &metric_css_classes(model.memory_usage),
                        set_height_request: 32,
                        set_width_request: 86,
                        set_halign: gtk::Align::Center,
                        set_valign: gtk::Align::Center,
                    },

                    // CPU（胶囊标签）
                    gtk::Label {
                        #[watch]
                        set_label: &format!("CPU {:>3}%", (model.cpu_usage * 100.0).round() as u32),
                        #[watch]
                        set_css_classes: &metric_css_classes(model.cpu_usage),
                        set_width_request: 86,
                        set_height_request: 32,
                        set_halign: gtk::Align::Center,
                        set_valign: gtk::Align::Center,
                    },

                    // 截图按钮
                    gtk::Button {
                        set_label: " s 1.0 ",
                        set_width_request: 60,
                        add_css_class: "screenshot-button",
                        connect_clicked => AppInput::Screenshot,
                    },

                    // 时间显示
                    gtk::Button {
                        #[watch]
                        set_label: &model.current_time,
                        set_width_request: 60,
                        add_css_class: "time-button",
                        connect_clicked => AppInput::ToggleSeconds,
                    },

                    // 监视器标签
                    gtk::Label {
                        #[watch]
                        set_text: &monitor_num_to_icon(model.monitor_num),
                        set_width_request: 40,
                        set_halign: gtk::Align::Center,
                    },
                },
            }
        }
    }

    fn init(
        shared_path: Self::Init,
        _root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        // 构建 Factory
        let tabs = FactoryVecDeque::<TabItem>::builder()
            .launch(gtk::Box::default())
            .forward(sender.input_sender(), |out| match out {
                TabOutput::Clicked(i) => AppInput::TabSelected(i),
            });

        let mut model = AppModel::new(shared_path.clone(), tabs);

        // 预创建9个“空状态”tab，先显示占位
        {
            let mut guard = model.tabs.guard();
            for i in 0..9 {
                guard.push_back(TabInit {
                    index: i,
                    emoji: pick_emoji(i).to_string(),
                    status: None,
                });
            }
        }

        // 应用CSS样式
        load_css();

        // 首帧初始化
        model.update_time_display();
        sender.input(AppInput::SystemUpdate);

        // 启动后台任务
        spawn_background_tasks(sender.clone(), shared_path);

        // Factory 父容器
        let tabs_box = model.tabs.widget();
        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
        self.reset();
        match msg {
            AppInput::TabSelected(index) => {
                info!("Tab selected: {}", index);
                self.active_tab = index;
                self.send_tag_command(true);
            }

            AppInput::LayoutChanged(layout_index) => {
                info!("Layout changed: {}", layout_index);
                self.send_layout_command(layout_index);
            }

            AppInput::ToggleSeconds => {
                self.show_seconds = !self.show_seconds;
                self.update_time_display();
            }

            AppInput::Screenshot => {
                info!("Taking screenshot");
                if let Err(e) = std::process::Command::new("flameshot").arg("gui").spawn() {
                    error!("Failed to launch flameshot: {}", e);
                }
            }

            AppInput::SharedMessageReceived(message) => {
                info!("SharedMessageReceived: {:?}", message);
                self.process_shared_message(message);

                // 用最新的 tag_status_vec 重建 9 个 Tab（简单可靠）
                let statuses = self.tag_status_vec.clone();
                let mut guard = self.tabs.guard();
                guard.clear();
                for i in 0..9 {
                    let s_opt = statuses.get(i).cloned();
                    guard.push_back(TabInit {
                        index: i,
                        emoji: pick_emoji(i).to_string(),
                        status: s_opt,
                    });
                }
            }

            AppInput::SystemUpdate => {
                self.system_monitor.update_if_needed();

                if let Some(snapshot) = self.system_monitor.get_snapshot() {
                    let total = snapshot.memory_available + snapshot.memory_used;
                    if total > 0 {
                        self.memory_usage = snapshot.memory_used as f64 / total as f64;
                    } else {
                        self.memory_usage = 0.0;
                    }
                    self.cpu_usage = (snapshot.cpu_average as f64 / 100.0).clamp(0.0, 1.0);
                }
            }

            AppInput::UpdateTime => {
                self.update_time_display();
            }
        }
    }
}

impl AppModel {
    fn new(shared_path: String, tabs: FactoryVecDeque<TabItem>) -> Self {
        let shared_buffer_opt = SharedRingBuffer::create_shared_ring_buffer(&shared_path);
        Self {
            active_tab: 0,
            layout_symbol: " ? ".to_string(),
            monitor_num: 0,
            show_seconds: false,
            tag_status_vec: Vec::new(),
            last_shared_message: None,
            memory_usage: 0.0,
            cpu_usage: 0.0,
            current_time: "".to_string(),
            shared_buffer_opt,
            system_monitor: SystemMonitor::new(1),
            tracker: 0,
            tabs,
        }
    }

    fn update_time_display(&mut self) {
        let now = Local::now();
        let format_str = if self.show_seconds {
            "%Y-%m-%d %H:%M:%S"
        } else {
            "%Y-%m-%d %H:%M"
        };
        self.current_time = now.format(format_str).to_string();
    }

    fn send_tag_command(&self, is_view: bool) {
        if let Some(shared_buffer) = &self.shared_buffer_opt {
            if let Some(ref message) = self.last_shared_message {
                let command = if is_view {
                    SharedCommand::view_tag(1 << self.active_tab, message.monitor_info.monitor_num)
                } else {
                    SharedCommand::toggle_tag(
                        1 << self.active_tab,
                        message.monitor_info.monitor_num,
                    )
                };

                if let Err(e) = shared_buffer.send_command(command) {
                    error!("Failed to send tag command: {}", e);
                }
            }
        }
    }

    fn send_layout_command(&self, layout_index: u32) {
        if let Some(shared_buffer) = &self.shared_buffer_opt {
            if let Some(ref message) = self.last_shared_message {
                let monitor_id = message.monitor_info.monitor_num;
                let command = SharedCommand::new(CommandType::SetLayout, layout_index, monitor_id);
                if let Err(e) = shared_buffer.send_command(command) {
                    error!("Failed to send layout command: {}", e);
                }
            }
        }
    }

    #[allow(dead_code)]
    fn resize_window_to_monitor(
        &self,
        window: Window,
        expected_x: i32,
        expected_y: i32,
        expected_width: i32,
        expected_height: i32,
    ) {
        let current_width = window.width();
        let current_height = window.height();
        info!(
            "Resizing window: {}x{} -> {}x{}",
            current_width, current_height, expected_width, expected_height
        );
        window.set_default_size(expected_width, expected_height);
        let display = gtk::gdk::Display::default().unwrap();
        unsafe {
            if let Some(x11_display) = display.downcast_ref::<gdk4_x11::X11Display>() {
                let xdisplay = x11_display.xdisplay();
                let surface = window.surface().unwrap();
                if let Some(x11_surface) = surface.downcast_ref::<gdk4_x11::X11Surface>() {
                    let xwindow = x11_surface.xid();
                    XMoveWindow(xdisplay as *mut _, xwindow, expected_x, expected_y);
                    XFlush(xdisplay as *mut _);
                }
            }
        }
    }

    fn process_shared_message(&mut self, message: SharedMessage) {
        self.last_shared_message = Some(message.clone());
        self.layout_symbol = message.monitor_info.get_ltsymbol();
        self.monitor_num = message.monitor_info.monitor_num as u8;
        self.set_tag_status_vec(message.monitor_info.tag_status_vec.to_vec());

        // 更新活动标签
        for (index, tag_status) in message.monitor_info.tag_status_vec.iter().enumerate() {
            if tag_status.is_selected {
                self.active_tab = index;
            }
        }
    }
}

// 后台任务：glib 定时器 + 共享内存线程
fn spawn_background_tasks(sender: ComponentSender<AppModel>, shared_path: String) {
    // 系统监控任务（每2秒）
    let sender1 = sender.clone();
    glib::timeout_add_seconds_local(2, move || {
        sender1.input(AppInput::SystemUpdate);
        ControlFlow::Continue
    });

    // 时间更新任务（每1秒）
    let sender2 = sender.clone();
    glib::timeout_add_seconds_local(1, move || {
        sender2.input(AppInput::UpdateTime);
        ControlFlow::Continue
    });

    // 共享内存任务
    let sender3 = sender.clone();
    std::thread::spawn(move || {
        shared_memory_worker(shared_path, sender3);
    });
}

// 共享内存工作器
fn shared_memory_worker(shared_path: String, sender: ComponentSender<AppModel>) {
    info!("Starting shared memory worker");
    let shared_buffer_opt: Option<SharedRingBuffer> = if shared_path.is_empty() {
        warn!("No shared path provided, running without shared memory");
        None
    } else {
        match SharedRingBuffer::open(&shared_path, None) {
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
    if let Some(ref shared_buffer) = shared_buffer_opt {
        loop {
            match shared_buffer.wait_for_message(Some(Duration::from_secs(2))) {
                Ok(true) => {
                    if let Ok(Some(message)) = shared_buffer.try_read_latest_message() {
                        if prev_timestamp != message.timestamp.into() {
                            prev_timestamp = message.timestamp.into();
                            sender.input(AppInput::SharedMessageReceived(message));
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

// 工具函数
fn monitor_num_to_icon(monitor_num: u8) -> String {
    match monitor_num {
        0 => "🥇".to_string(),
        1 => "🥈".to_string(),
        2 => "🥉".to_string(),
        _ => "❔".to_string(),
    }
}

fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(include_str!("styles.css"));
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

fn initialize_logging(shared_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let now = Local::now();
    let timestamp = now.format("%Y-%m-%d_%H_%M_%S").to_string();

    let file_name = if shared_path.is_empty() {
        "relm_bar".to_string()
    } else {
        std::path::Path::new(shared_path)
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| format!("relm_bar_{}", name))
            .unwrap_or_else(|| "relm_bar".to_string())
    };

    let log_filename = format!("{}_{}", file_name, timestamp);

    Logger::try_with_str("info")?
        .format(flexi_logger::colored_opt_format)
        .log_to_file(
            FileSpec::default()
                .directory("/var/tmp/jwm")
                .basename(log_filename)
                .suffix("log"),
        )
        .duplicate_to_stdout(Duplicate::Debug)
        .rotate(
            Criterion::Size(10_000_000),
            Naming::Numbers,
            Cleanup::KeepLogFiles(5),
        )
        .start()?;

    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let shared_path = args.get(1).cloned().unwrap_or_default();

    if let Err(e) = initialize_logging(&shared_path) {
        eprintln!("Init logging failed: {e}");
    }

    // 构建稳定 App ID
    let mut instance_name = shared_path.replace("/dev/shm/monitor_", "relm_bar_");
    if instance_name.is_empty() {
        instance_name = "relm_bar".to_string();
    }
    instance_name = format!("dev.you.{}", instance_name.replace('/', "_"));

    info!("App ID: {}", instance_name);
    let app = RelmApp::new(&instance_name).with_args(vec![]);
    app.run::<AppModel>(shared_path);
}
