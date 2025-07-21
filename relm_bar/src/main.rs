// main.rs
use relm4::prelude::*;
use adw::prelude::*;
use gtk::prelude::*;
use cairo::Context;
use chrono::Local;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use std::time::Duration;
use log::{error, info, warn};
use flexi_logger::{Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming};

// 假设这些是外部依赖，需要根据实际情况调整
// use shared_structures::{CommandType, SharedCommand, SharedMessage, SharedRingBuffer, TagStatus};

// 为了演示，这里定义简化的结构体
#[derive(Debug, Clone)]
pub struct TagStatus {
    pub is_selected: bool,
    pub is_occ: bool,
    pub is_filled: bool,
    pub is_urg: bool,
}

#[derive(Debug, Clone)]
pub struct MonitorInfo {
    pub monitor_num: u32,
    pub monitor_x: i32,
    pub monitor_y: i32,
    pub monitor_width: i32,
    pub monitor_height: i32,
    pub border_w: i32,
    pub ltsymbol: String,
    pub tag_status_vec: Vec<TagStatus>,
}

#[derive(Debug, Clone)]
pub struct SharedMessage {
    pub timestamp: u128,
    pub monitor_info: MonitorInfo,
}

#[derive(Debug, Clone)]
pub struct SharedCommand {
    pub command_type: u32,
    pub value: u32,
    pub monitor_id: u32,
}

impl SharedCommand {
    pub fn new(command_type: u32, value: u32, monitor_id: u32) -> Self {
        Self { command_type, value, monitor_id }
    }
    
    pub fn view_tag(tag_bit: u32, monitor_id: u32) -> Self {
        Self::new(1, tag_bit, monitor_id)
    }
    
    pub fn toggle_tag(tag_bit: u32, monitor_id: u32) -> Self {
        Self::new(2, tag_bit, monitor_id)
    }
}

// 系统监控结构
#[derive(Debug, Clone)]
pub struct SystemSnapshot {
    pub memory_used: u64,
    pub memory_available: u64,
    pub cpu_average: f32,
}

pub struct SystemMonitor {
    last_update: std::time::Instant,
    update_interval: Duration,
    snapshot: Option<SystemSnapshot>,
}

impl SystemMonitor {
    pub fn new(interval_secs: u64) -> Self {
        Self {
            last_update: std::time::Instant::now() - Duration::from_secs(interval_secs),
            update_interval: Duration::from_secs(interval_secs),
            snapshot: None,
        }
    }
    
    pub fn update_if_needed(&mut self) {
        if self.last_update.elapsed() >= self.update_interval {
            self.update();
        }
    }
    
    pub fn update(&mut self) {
        // 模拟系统数据获取
        self.snapshot = Some(SystemSnapshot {
            memory_used: 4_000_000_000,
            memory_available: 8_000_000_000,
            cpu_average: (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() % 100) as f32,
        });
        self.last_update = std::time::Instant::now();
    }
    
    pub fn get_snapshot(&self) -> Option<&SystemSnapshot> {
        self.snapshot.as_ref()
    }
}

// 音频管理器
pub struct AudioManager {
    last_update: std::time::Instant,
    update_interval: Duration,
}

impl AudioManager {
    pub fn new() -> Self {
        Self {
            last_update: std::time::Instant::now(),
            update_interval: Duration::from_secs(1),
        }
    }
    
    pub fn update_if_needed(&mut self) {
        if self.last_update.elapsed() >= self.update_interval {
            self.last_update = std::time::Instant::now();
            // 音频更新逻辑
        }
    }
}

// 应用状态
pub struct AppState {
    pub active_tab: usize,
    pub layout_symbol: String,
    pub monitor_num: u8,
    pub show_seconds: bool,
    pub tag_status_vec: Vec<TagStatus>,
    pub system_monitor: SystemMonitor,
    pub audio_manager: AudioManager,
    pub last_shared_message: Option<SharedMessage>,
    pub pending_messages: Vec<SharedMessage>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            active_tab: 0,
            layout_symbol: " ? ".to_string(),
            monitor_num: 0,
            show_seconds: false,
            tag_status_vec: Vec::new(),
            system_monitor: SystemMonitor::new(1),
            audio_manager: AudioManager::new(),
            last_shared_message: None,
            pending_messages: Vec::new(),
        }
    }
}

// 主应用消息
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

// 主应用模型
pub struct App {
    state: Arc<Mutex<AppState>>,
    command_sender: Option<mpsc::UnboundedSender<SharedCommand>>,
    memory_usage: f64,
    cpu_usage: f64,
    current_time: String,
}

#[relm4::component(pub)]
impl SimpleComponent for App {
    type Init = String; // 共享路径
    type Input = AppInput;
    type Output = ();

    view! {
        #[root]
        adw::ApplicationWindow {
            set_decorated: false,
            set_default_size: (1000, 40),
            set_resizable: true,
            add_css_class: "main-window",

            gtk::Box {
                set_orientation: gtk::Orientation::Horizontal,
                set_spacing: 3,
                set_margin_all: 3,

                // 标签栏
                gtk::Box {
                    set_orientation: gtk::Orientation::Horizontal,
                    set_spacing: 3,

                    #[name = "tab_button_0"]
                    gtk::Button {
                        set_label: "🍜",
                        set_width_request: 40,
                        add_css_class: "tab-button",
                        connect_clicked[sender] => move |_| {
                            sender.input(AppInput::TabSelected(0));
                        },
                    },

                    #[name = "tab_button_1"]
                    gtk::Button {
                        set_label: "🎨",
                        set_width_request: 40,
                        add_css_class: "tab-button",
                        connect_clicked[sender] => move |_| {
                            sender.input(AppInput::TabSelected(1));
                        },
                    },

                    #[name = "tab_button_2"]
                    gtk::Button {
                        set_label: "🍀",
                        set_width_request: 40,
                        add_css_class: "tab-button",
                        connect_clicked[sender] => move |_| {
                            sender.input(AppInput::TabSelected(2));
                        },
                    },

                    #[name = "tab_button_3"]
                    gtk::Button {
                        set_label: "🧿",
                        set_width_request: 40,
                        add_css_class: "tab-button",
                        connect_clicked[sender] => move |_| {
                            sender.input(AppInput::TabSelected(3));
                        },
                    },

                    #[name = "tab_button_4"]
                    gtk::Button {
                        set_label: "🌟",
                        set_width_request: 40,
                        add_css_class: "tab-button",
                        connect_clicked[sender] => move |_| {
                            sender.input(AppInput::TabSelected(4));
                        },
                    },

                    #[name = "tab_button_5"]
                    gtk::Button {
                        set_label: "🐐",
                        set_width_request: 40,
                        add_css_class: "tab-button",
                        connect_clicked[sender] => move |_| {
                            sender.input(AppInput::TabSelected(5));
                        },
                    },

                    #[name = "tab_button_6"]
                    gtk::Button {
                        set_label: "🏆",
                        set_width_request: 40,
                        add_css_class: "tab-button",
                        connect_clicked[sender] => move |_| {
                            sender.input(AppInput::TabSelected(6));
                        },
                    },

                    #[name = "tab_button_7"]
                    gtk::Button {
                        set_label: "🕊️",
                        set_width_request: 40,
                        add_css_class: "tab-button",
                        connect_clicked[sender] => move |_| {
                            sender.input(AppInput::TabSelected(7));
                        },
                    },

                    #[name = "tab_button_8"]
                    gtk::Button {
                        set_label: "🏡",
                        set_width_request: 40,
                        add_css_class: "tab-button",
                        connect_clicked[sender] => move |_| {
                            sender.input(AppInput::TabSelected(8));
                        },
                    },
                },

                // 布局标签
                #[name = "layout_label"]
                gtk::Label {
                    set_text: &model.get_layout_symbol(),
                    set_width_request: 40,
                    set_halign: gtk::Align::Center,
                    add_css_class: "layout-label",
                },

                // 布局按钮
                gtk::ScrolledWindow {
                    set_hscrollbar_policy: gtk::PolicyType::Automatic,
                    set_vscrollbar_policy: gtk::PolicyType::Never,
                    set_width_request: 60,

                    gtk::Box {
                        set_orientation: gtk::Orientation::Horizontal,
                        set_spacing: 5,

                        gtk::Button {
                            set_label: "[]=",
                            set_width_request: 40,
                            add_css_class: "layout-button",
                            connect_clicked[sender] => move |_| {
                                sender.input(AppInput::LayoutChanged(0));
                            },
                        },

                        gtk::Button {
                            set_label: "<><",
                            set_width_request: 40,
                            add_css_class: "layout-button",
                            connect_clicked[sender] => move |_| {
                                sender.input(AppInput::LayoutChanged(1));
                            },
                        },

                        gtk::Button {
                            set_label: "[M]",
                            set_width_request: 40,
                            add_css_class: "layout-button",
                            connect_clicked[sender] => move |_| {
                                sender.input(AppInput::LayoutChanged(2));
                            },
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

                    // 内存进度条
                    gtk::Box {
                        set_orientation: gtk::Orientation::Vertical,
                        set_spacing: 10,

                        #[name = "memory_progress"]
                        gtk::ProgressBar {
                            set_halign: gtk::Align::Center,
                            set_valign: gtk::Align::Center,
                            set_vexpand: true,
                            set_width_request: 200,
                            set_fraction: model.memory_usage,
                            add_css_class: "neon-progress",
                        },
                    },

                    // CPU使用率绘制区域
                    #[name = "cpu_drawing_area"]
                    gtk::DrawingArea {
                        set_width_request: 64,
                        set_draw_func[model.cpu_usage] => move |_, ctx, width, height| {
                            draw_cpu_usage(ctx, width, height, cpu_usage);
                        },
                    },

                    // 截图按钮
                    gtk::Button {
                        set_label: " s 1.0 ",
                        set_width_request: 60,
                        add_css_class: "screenshot-button",
                        connect_clicked[sender] => move |_| {
                            sender.input(AppInput::Screenshot);
                        },
                    },

                    // 时间显示
                    #[name = "time_label"]
                    gtk::Button {
                        set_label: &model.current_time,
                        set_width_request: 60,
                        add_css_class: "time-button",
                        connect_clicked[sender] => move |_| {
                            sender.input(AppInput::ToggleSeconds);
                        },
                    },

                    // 监视器标签
                    #[name = "monitor_label"]
                    gtk::Label {
                        set_text: &model.get_monitor_icon(),
                        set_width_request: 40,
                        set_halign: gtk::Align::Center,
                    },
                },
            }
        }
    }

    fn init(
        shared_path: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        // 初始化日志
        if let Err(e) = initialize_logging(&shared_path) {
            eprintln!("Failed to initialize logging: {}", e);
        }

        info!("Starting GTK4 Bar with Relm4 v1.0");
        info!("Shared path: {}", shared_path);

        // 初始化状态
        let state = Arc::new(Mutex::new(AppState::new()));

        // 创建命令通道
        let (command_sender, command_receiver) = mpsc::unbounded_channel();

        let model = App {
            state: state.clone(),
            command_sender: Some(command_sender),
            memory_usage: 0.0,
            cpu_usage: 0.0,
            current_time: String::new(),
        };

        // 应用CSS样式
        load_css();

        // 启动后台任务
        spawn_background_tasks(sender.clone(), state.clone(), shared_path, command_receiver);

        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            AppInput::TabSelected(index) => {
                info!("Tab selected: {}", index);
                if let Ok(mut state) = self.state.lock() {
                    state.active_tab = index;
                }
                self.send_tag_command(true);
                self.update_tab_styles(&sender);
            }

            AppInput::LayoutChanged(layout_index) => {
                info!("Layout changed: {}", layout_index);
                self.send_layout_command(layout_index);
            }

            AppInput::ToggleSeconds => {
                if let Ok(mut state) = self.state.lock() {
                    state.show_seconds = !state.show_seconds;
                }
                self.update_time_display();
            }

            AppInput::Screenshot => {
                info!("Taking screenshot");
                std::process::Command::new("flameshot")
                    .arg("gui")
                    .spawn()
                    .ok();
            }

            AppInput::SharedMessageReceived(message) => {
                self.process_shared_message(message);
                self.update_tab_styles(&sender);
            }

            AppInput::SystemUpdate => {
                if let Ok(mut state) = self.state.lock() {
                    state.system_monitor.update_if_needed();
                    state.audio_manager.update_if_needed();
                    
                    if let Some(snapshot) = state.system_monitor.get_snapshot() {
                        let total = snapshot.memory_available + snapshot.memory_used;
                        self.memory_usage = snapshot.memory_used as f64 / total as f64;
                        self.cpu_usage = snapshot.cpu_average as f64 / 100.0;
                    }
                }
            }

            AppInput::UpdateTime => {
                self.update_time_display();
            }
        }
    }
}

impl App {
    fn get_layout_symbol(&self) -> String {
        if let Ok(state) = self.state.lock() {
            state.layout_symbol.clone()
        } else {
            " ? ".to_string()
        }
    }

    fn get_monitor_icon(&self) -> String {
        if let Ok(state) = self.state.lock() {
            monitor_num_to_icon(state.monitor_num)
        } else {
            "?".to_string()
        }
    }

    fn update_time_display(&mut self) {
        let show_seconds = if let Ok(state) = self.state.lock() {
            state.show_seconds
        } else {
            false
        };

        let now = Local::now();
        let format_str = if show_seconds {
            "%Y-%m-%d %H:%M:%S"
        } else {
            "%Y-%m-%d %H:%M"
        };
        self.current_time = now.format(format_str).to_string();
    }

    fn send_tag_command(&self, is_view: bool) {
        if let (Ok(state), Some(ref sender)) = (self.state.lock(), &self.command_sender) {
            if let Some(ref message) = state.last_shared_message {
                let command = if is_view {
                    SharedCommand::view_tag(1 << state.active_tab, message.monitor_info.monitor_num)
                } else {
                    SharedCommand::toggle_tag(1 << state.active_tab, message.monitor_info.monitor_num)
                };
                
                if let Err(e) = sender.send(command) {
                    error!("Failed to send tag command: {}", e);
                }
            }
        }
    }

    fn send_layout_command(&self, layout_index: u32) {
        if let (Ok(state), Some(ref sender)) = (self.state.lock(), &self.command_sender) {
            if let Some(ref message) = state.last_shared_message {
                let command = SharedCommand::new(3, layout_index, message.monitor_info.monitor_num);
                if let Err(e) = sender.send(command) {
                    error!("Failed to send layout command: {}", e);
                }
            }
        }
    }

    fn process_shared_message(&mut self, message: SharedMessage) {
        if let Ok(mut state) = self.state.lock() {
            state.last_shared_message = Some(message.clone());
            state.layout_symbol = message.monitor_info.ltsymbol.clone();
            state.monitor_num = message.monitor_info.monitor_num as u8;
            state.tag_status_vec = message.monitor_info.tag_status_vec.clone();

            // 更新活动标签
            for (index, tag_status) in message.monitor_info.tag_status_vec.iter().enumerate() {
                if tag_status.is_selected {
                    state.active_tab = index;
                    break;
                }
            }
        }
    }

    fn update_tab_styles(&self, _sender: &ComponentSender<Self>) {
        // 在Relm4中，样式更新需要通过重新渲染来实现
        // 这里暂时留空，实际实现需要更复杂的机制
        info!("Updating tab styles");
    }
}

// CPU绘制函数
fn draw_cpu_usage(ctx: &Context, width: i32, height: i32, cpu_usage: f64) {
    let width_f = width as f64;
    let height_f = height as f64;

    // 清除背景
    ctx.set_source_rgba(0.0, 0.0, 0.0, 0.0);
    ctx.paint().unwrap();

    // 绘制背景
    ctx.set_source_rgba(0.0, 0.0, 0.0, 0.3);
    ctx.rectangle(0.0, 0.0, width_f, height_f);
    ctx.fill().unwrap();

    // 绘制CPU使用率条
    let used_height = height_f * cpu_usage;
    let y_offset = height_f - used_height;

    // 设置渐变色
    let gradient = cairo::LinearGradient::new(0.0, 0.0, 0.0, height_f);
    gradient.add_color_stop_rgba(0.0, 1.0, 0.0, 0.0, 0.9);
    gradient.add_color_stop_rgba(0.5, 1.0, 1.0, 0.0, 0.9);
    gradient.add_color_stop_rgba(1.0, 0.0, 1.0, 1.0, 0.9);

    ctx.set_source(&gradient).unwrap();
    ctx.rectangle(0.0, y_offset, width_f, used_height);
    ctx.fill().unwrap();
}

// 后台任务
fn spawn_background_tasks(
    sender: ComponentSender<App>,
    state: Arc<Mutex<AppState>>,
    shared_path: String,
    mut command_receiver: mpsc::UnboundedReceiver<SharedCommand>,
) {
    // 系统监控任务
    let sender_clone = sender.clone();
    relm4::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        loop {
            interval.tick().await;
            sender_clone.input(AppInput::SystemUpdate);
        }
    });

    // 时间更新任务
    let sender_clone = sender.clone();
    relm4::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            sender_clone.input(AppInput::UpdateTime);
        }
    });

    // 共享内存任务
    let sender_clone = sender.clone();
    relm4::spawn(async move {
        shared_memory_worker(shared_path, state, sender_clone, command_receiver).await;
    });
}

// 共享内存工作器
async fn shared_memory_worker(
    shared_path: String,
    state: Arc<Mutex<AppState>>,
    sender: ComponentSender<App>,
    mut command_receiver: mpsc::UnboundedReceiver<SharedCommand>,
) {
    info!("Starting shared memory worker");
    
    let mut interval = tokio::time::interval(Duration::from_millis(10));
    
    loop {
        tokio::select! {
            _ = interval.tick() => {
                // 处理待处理的消息
                if let Ok(mut state) = state.lock() {
                    let messages = state.pending_messages.drain(..).collect::<Vec<_>>();
                    for message in messages {
                        sender.input(AppInput::SharedMessageReceived(message));
                    }
                }
                
                // 模拟接收共享内存消息
                // 实际实现需要替换为真实的共享内存读取逻辑
            }
            
            command = command_receiver.recv() => {
                if let Some(cmd) = command {
                    info!("Processing command: {:?}", cmd);
                    // 实际实现需要发送到共享内存
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
        _ => "?".to_string(),
    }
}

fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(CSS_STYLES);
    
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

const CSS_STYLES: &str = r#"
window {
  background-color: rgba(255, 255, 255, 0.9);
}

.tab-button {
  margin: 0px 2px;
  padding: 0px;
  border-radius: 6px;
  font-size: 20px;
  border: 1px solid transparent;
  background-image: none;
  color: #333;
  transition: all 0.2s ease;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.1);
}

.tab-button:hover {
  transform: scale(1.02);
  box-shadow: 0 2px 6px rgba(0, 0, 0, 0.15);
  transition: all 0.2s ease;
}

.time-button {
  border-radius: 2px;
  border: 1px solid white;
  margin: 0px 2px;
  padding: 0px;
  background-color: rgba(255, 254, 253, 0.8);
  background-image: none;
}

.time-button:hover {
  background-color: cyan;
  background-image: none;
  color: darkorange;
}

.layout-button {
  margin: 0px 2px;
  padding: 0px;
  font-size: 12px;
}

.screenshot-button {
  margin: 0px 2px;
  padding: 0px;
  border-radius: 2px;
  border: 0.5px solid white;
  background-color: rgba(255, 254, 253, 0.8);
  background-image: none;
}

.screenshot-button:hover {
  background-color: cyan;
  background-image: none;
  color: darkorange;
}

.layout-label {
  color: orange;
}

.neon-progress progress {
  background: linear-gradient(to left, #ff00ff, #00ffff);
  border-radius: 1px;
}
"#;

fn initialize_logging(shared_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let now = Local::now();
    let timestamp = now.format("%Y-%m-%d_%H_%M_%S").to_string();

    let file_name = if shared_path.is_empty() {
        "gtk_bar_relm4".to_string()
    } else {
        std::path::Path::new(shared_path)
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| format!("gtk_bar_relm4_{}", name))
            .unwrap_or_else(|| "gtk_bar_relm4".to_string())
    };

    let log_filename = format!("{}_{}", file_name, timestamp);

    Logger::try_with_str("info")?
        .format(flexi_logger::colored_opt_format)
        .log_to_file(
            FileSpec::default()
                .directory("/tmp")
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

    let app = RelmApp::new("com.example.gtk_bar_relm4");
    app.run::<App>(shared_path);
}
