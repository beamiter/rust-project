// main.rs
use cairo::Context;
use chrono::Local;
use flexi_logger::{Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming};
use gdk4_x11::x11::xlib::{XFlush, XMoveWindow};
use gtk::prelude::*;
use gtk4::Window;
use log::{error, info, warn};
use relm4::abstractions::DrawHandler;
use relm4::prelude::*;
use relm4::{ComponentParts, ComponentSender, RelmApp, RelmWidgetExt, SimpleComponent};
use std::time::Duration;

mod audio_manager;
mod error;
mod system_monitor;

use error::AppError;
use shared_structures::{CommandType, SharedCommand, SharedMessage, SharedRingBuffer, TagStatus};
use system_monitor::SystemMonitor;

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
    #[do_not_track]
    handler: DrawHandler,
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

                // 标签栏
                gtk::Box {
                    set_orientation: gtk::Orientation::Horizontal,
                    set_spacing: 3,

                    #[name = "tab_button_0"]
                    gtk::Button {
                        set_label: "🍜",
                        set_width_request: 40,
                        add_css_class: "tab-button",
                        connect_clicked => AppInput::TabSelected(0),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("occupied", model.tag_status_vec.get(0).map_or(false, |s| s.is_occ)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("urgent", model.tag_status_vec.get(0).map_or(false, |s| s.is_urg)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("filled", model.tag_status_vec.get(0).map_or(false, |s| s.is_filled)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("selected", model.tag_status_vec.get(0).map_or(false, |s| s.is_selected)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("empty", model.tag_status_vec.get(0).map_or(false, |s| !(s.is_urg || s.is_filled || s.is_selected || s.is_occ))),
                    },

                    #[name = "tab_button_1"]
                    gtk::Button {
                        set_label: "🎨",
                        set_width_request: 40,
                        add_css_class: "tab-button",
                        connect_clicked => AppInput::TabSelected(1),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("occupied", model.tag_status_vec.get(1).map_or(false, |s| s.is_occ)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("urgent", model.tag_status_vec.get(1).map_or(false, |s| s.is_urg)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("filled", model.tag_status_vec.get(1).map_or(false, |s| s.is_filled)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("selected", model.tag_status_vec.get(1).map_or(false, |s| s.is_selected)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("empty", model.tag_status_vec.get(1).map_or(false, |s| !(s.is_urg || s.is_filled || s.is_selected || s.is_occ))),
                    },

                    #[name = "tab_button_2"]
                    gtk::Button {
                        set_label: "🍀",
                        set_width_request: 40,
                        add_css_class: "tab-button",
                        connect_clicked => AppInput::TabSelected(2),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("occupied", model.tag_status_vec.get(2).map_or(false, |s| s.is_occ)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("urgent", model.tag_status_vec.get(2).map_or(false, |s| s.is_urg)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("filled", model.tag_status_vec.get(2).map_or(false, |s| s.is_filled)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("selected", model.tag_status_vec.get(2).map_or(false, |s| s.is_selected)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("empty", model.tag_status_vec.get(2).map_or(false, |s| !(s.is_urg || s.is_filled || s.is_selected || s.is_occ))),
                    },

                    #[name = "tab_button_3"]
                    gtk::Button {
                        set_label: "🧿",
                        set_width_request: 40,
                        add_css_class: "tab-button",
                        connect_clicked => AppInput::TabSelected(3),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("occupied", model.tag_status_vec.get(3).map_or(false, |s| s.is_occ)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("urgent", model.tag_status_vec.get(3).map_or(false, |s| s.is_urg)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("filled", model.tag_status_vec.get(3).map_or(false, |s| s.is_filled)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("selected", model.tag_status_vec.get(3).map_or(false, |s| s.is_selected)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("empty", model.tag_status_vec.get(3).map_or(false, |s| !(s.is_urg || s.is_filled || s.is_selected || s.is_occ))),
                    },

                    #[name = "tab_button_4"]
                    gtk::Button {
                        set_label: "🌟",
                        set_width_request: 40,
                        add_css_class: "tab-button",
                        connect_clicked => AppInput::TabSelected(4),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("occupied", model.tag_status_vec.get(4).map_or(false, |s| s.is_occ)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("urgent", model.tag_status_vec.get(4).map_or(false, |s| s.is_urg)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("filled", model.tag_status_vec.get(4).map_or(false, |s| s.is_filled)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("selected", model.tag_status_vec.get(4).map_or(false, |s| s.is_selected)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("empty", model.tag_status_vec.get(4).map_or(false, |s| !(s.is_urg || s.is_filled || s.is_selected || s.is_occ))),
                    },

                    #[name = "tab_button_5"]
                    gtk::Button {
                        set_label: "🐐",
                        set_width_request: 40,
                        add_css_class: "tab-button",
                        connect_clicked => AppInput::TabSelected(5),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("occupied", model.tag_status_vec.get(5).map_or(false, |s| s.is_occ)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("urgent", model.tag_status_vec.get(5).map_or(false, |s| s.is_urg)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("filled", model.tag_status_vec.get(5).map_or(false, |s| s.is_filled)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("selected", model.tag_status_vec.get(5).map_or(false, |s| s.is_selected)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("empty", model.tag_status_vec.get(5).map_or(false, |s| !(s.is_urg || s.is_filled || s.is_selected || s.is_occ))),
                    },

                    #[name = "tab_button_6"]
                    gtk::Button {
                        set_label: "🏆",
                        set_width_request: 40,
                        add_css_class: "tab-button",
                        connect_clicked => AppInput::TabSelected(6),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("occupied", model.tag_status_vec.get(6).map_or(false, |s| s.is_occ)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("urgent", model.tag_status_vec.get(6).map_or(false, |s| s.is_urg)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("filled", model.tag_status_vec.get(6).map_or(false, |s| s.is_filled)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("selected", model.tag_status_vec.get(6).map_or(false, |s| s.is_selected)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("empty", model.tag_status_vec.get(6).map_or(false, |s| !(s.is_urg || s.is_filled || s.is_selected || s.is_occ))),
                    },

                    #[name = "tab_button_7"]
                    gtk::Button {
                        set_label: "🕊️",
                        set_width_request: 40,
                        add_css_class: "tab-button",
                        connect_clicked => AppInput::TabSelected(7),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("occupied", model.tag_status_vec.get(7).map_or(false, |s| s.is_occ)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("urgent", model.tag_status_vec.get(7).map_or(false, |s| s.is_urg)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("filled", model.tag_status_vec.get(7).map_or(false, |s| s.is_filled)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("selected", model.tag_status_vec.get(7).map_or(false, |s| s.is_selected)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("empty", model.tag_status_vec.get(7).map_or(false, |s| !(s.is_urg || s.is_filled || s.is_selected || s.is_occ))),
                    },

                    #[name = "tab_button_8"]
                    gtk::Button {
                        set_label: "🏡",
                        set_width_request: 40,
                        add_css_class: "tab-button",
                        connect_clicked => AppInput::TabSelected(8),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("occupied", model.tag_status_vec.get(8).map_or(false, |s| s.is_occ)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("urgent", model.tag_status_vec.get(8).map_or(false, |s| s.is_urg)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("filled", model.tag_status_vec.get(8).map_or(false, |s| s.is_filled)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("selected", model.tag_status_vec.get(8).map_or(false, |s| s.is_selected)),
                        #[track(model.changed(AppModel::tag_status_vec()))]
                        set_class_active: ("empty", model.tag_status_vec.get(8).map_or(false, |s| !(s.is_urg || s.is_filled || s.is_selected || s.is_occ))),
                    },
                },

                // 布局标签
                #[name = "layout_label"]
                gtk::Label {
                    #[watch]
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
                            connect_clicked => AppInput::LayoutChanged(0),
                        },

                        gtk::Button {
                            set_label: "<><",
                            set_width_request: 40,
                            add_css_class: "layout-button",
                            connect_clicked => AppInput::LayoutChanged(1),
                        },

                        gtk::Button {
                            set_label: "[M]",
                            set_width_request: 40,
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
                            #[watch]
                            set_fraction: model.memory_usage,
                            add_css_class: "neon-progress",
                        },
                    },

                    // CPU使用率绘制区域
                    #[local_ref]
                    area -> gtk::DrawingArea {
                      set_vexpand: true,
                      set_hexpand: true,
                      set_width_request: 64,
                    },

                    // 截图按钮
                    gtk::Button {
                        set_label: " s 1.0 ",
                        set_width_request: 60,
                        add_css_class: "screenshot-button",
                        connect_clicked => AppInput::Screenshot,
                    },

                    // 时间显示
                    #[name = "time_label"]
                    gtk::Button {
                        #[watch]
                        set_label: &model.current_time,
                        set_width_request: 60,
                        add_css_class: "time-button",
                        connect_clicked => AppInput::ToggleSeconds,
                    },

                    // 监视器标签
                    #[name = "monitor_label"]
                    gtk::Label {
                        #[watch]
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
            error!("Failed to initialize logging: {}", e);
            std::process::exit(1);
        }
        info!("Starting Relm4 Bar v1.0");
        info!("Shared path: {}", shared_path);

        // 创建命令通道
        let model = AppModel::new(shared_path.clone());
        let area = model.handler.drawing_area();

        // 应用CSS样式
        load_css();

        // 启动后台任务
        spawn_background_tasks(sender.clone(), shared_path);

        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
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
                std::process::Command::new("flameshot")
                    .arg("gui")
                    .spawn()
                    .ok();
            }

            AppInput::SharedMessageReceived(message) => {
                info!("SharedMessageReceived: {:?}", message);
                self.process_shared_message(message);
            }

            AppInput::SystemUpdate => {
                self.system_monitor.update_if_needed();

                if let Some(snapshot) = self.system_monitor.get_snapshot() {
                    let total = snapshot.memory_available + snapshot.memory_used;
                    self.memory_usage = snapshot.memory_used as f64 / total as f64;
                    self.cpu_usage = snapshot.cpu_average as f64 / 100.0;
                }
            }

            AppInput::UpdateTime => {
                self.update_time_display();
            }
        }

        let ctx = self.handler.get_context();
        draw_cpu_usage(
            &ctx,
            self.handler.width(),
            self.handler.height(),
            self.cpu_usage,
        );
    }
}

impl AppModel {
    pub fn new(shared_path: String) -> Self {
        let shared_buffer_opt = SharedRingBuffer::create_shared_ring_buffer(&shared_path);
        Self {
            active_tab: 0,
            layout_symbol: " ? ".to_string(),
            monitor_num: 0,
            show_seconds: false,
            tag_status_vec: Vec::new(),
            last_shared_message: None,
            memory_usage: 0.,
            cpu_usage: 0.,
            current_time: "".to_string(),
            shared_buffer_opt,
            system_monitor: SystemMonitor::new(1),
            tracker: 0,
            handler: DrawHandler::new(),
        }
    }

    fn get_monitor_icon(&self) -> String {
        monitor_num_to_icon(self.monitor_num)
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
        let display = gtk4::gdk::Display::default().unwrap();
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
        let app = relm4::main_application();
        if let Some(window) = app.active_window() {
            let _current_width = window.width();
            let _current_height = window.height();
        }
        // 更新活动标签
        for (index, tag_status) in message.monitor_info.tag_status_vec.iter().enumerate() {
            if tag_status.is_selected {
                self.active_tab = index;
            }
        }
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
    ctx.set_source_rgba(0.2, 0.2, 0.2, 0.3);
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
fn spawn_background_tasks(sender: ComponentSender<AppModel>, shared_path: String) {
    // 系统监控任务
    let sender_clone = sender.clone();
    relm4::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(1000));
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
        shared_memory_worker(shared_path, sender_clone).await;
    });
}

// 共享内存工作器
async fn shared_memory_worker(shared_path: String, sender: ComponentSender<AppModel>) {
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

    let mut interval = tokio::time::interval(Duration::from_millis(10));
    let mut prev_timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    loop {
        tokio::select! {
            _ = interval.tick() => {
                if let Some(ref shared_buffer) = shared_buffer_opt {
                    match shared_buffer.try_read_latest_message() {
                        Ok(Some(message)) => {
                            if prev_timestamp != message.timestamp.into() {
                                prev_timestamp = message.timestamp.into();
                                sender.input(AppInput::SharedMessageReceived(message));
                            }
                        }
                        Ok(_) => {}
                        Err(e) => {
                            error!("Ring buffer read error: {}", e);
                        }
                    }
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
    let mut instance_name = shared_path.replace("/dev/shm/monitor_", "relm_bar_");
    if instance_name.is_empty() {
        instance_name = "relm_bar".to_string();
    }
    instance_name = format!("{}.{}", instance_name, instance_name);
    info!("instance_name: {}", instance_name);
    let app = RelmApp::new(&instance_name).with_args(vec![]); // 传递空参数避免文件处理
    app.run::<AppModel>(shared_path);
}
