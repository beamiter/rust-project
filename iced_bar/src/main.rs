use chrono::Local;
use flexi_logger::{Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming};
use iced::daemon::Appearance;
use iced::time::{self};
use iced::widget::canvas::path::Arc;
use iced::widget::canvas::{Cache, Geometry, Path};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{Scrollable, Space, button, progress_bar, rich_text, row};
use iced::widget::{canvas, container};
use iced::widget::{mouse_area, span};
use iced::{gradient, mouse};

use iced::window::Id;
use iced::{
    Background, Border, Color, Degrees, Element, Length, Point, Radians, Rectangle, Renderer, Size,
    Subscription, Task, Theme, border, color,
    widget::{Column, Row, text},
    window,
};
mod error;
pub use error::AppError;
use iced_aw::{TabBar, TabLabel};
use iced_fonts::NERD_FONT_BYTES;
use log::{error, info, warn};
use shared_structures::{CommandType, SharedCommand, SharedMessage, SharedRingBuffer};
use std::env;
use std::process::Command;
use std::sync::{Once, mpsc};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::audio_manager::AudioManager;
use crate::system_monitor::SystemMonitor;

pub mod audio_manager;
pub mod system_monitor;

static START: Once = Once::new();

/// Monitor heartbeat from background thread
fn heartbeat_monitor(heartbeat_receiver: mpsc::Receiver<()>) {
    info!("Starting heartbeat monitor");

    loop {
        match heartbeat_receiver.recv_timeout(Duration::from_secs(5)) {
            Ok(_) => {
                // Heartbeat received, continue
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                error!("Shared memory thread heartbeat timeout");
                std::process::exit(1);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                error!("Shared memory thread disconnected");
                std::process::exit(1);
            }
        }
    }
}

fn shared_memory_worker(
    shared_path: String,
    message_sender: mpsc::Sender<SharedMessage>,
    heartbeat_sender: mpsc::Sender<()>,
    command_receiver: mpsc::Receiver<SharedCommand>,
) {
    info!("Starting shared memory worker thread");

    // 尝试打开或创建共享环形缓冲区
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

    let mut prev_timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();

    let mut frame_count: u128 = 0;
    let mut consecutive_errors = 0;

    loop {
        // 发送心跳信号
        if heartbeat_sender.send(()).is_err() {
            warn!("Heartbeat receiver disconnected");
            break;
        }

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
                    // info!("shared_buffer {:?}", message);
                    consecutive_errors = 0;
                    if prev_timestamp != message.timestamp {
                        prev_timestamp = message.timestamp;
                        if let Err(e) = message_sender.send(message) {
                            error!("Failed to send message: {}", e);
                            break;
                        } else {
                            info!("send message ok");
                        }
                    }
                }
                Ok(None) => {
                    consecutive_errors = 0;
                }
                Err(e) => {
                    consecutive_errors += 1;
                    if frame_count % 1000 == 0 || consecutive_errors == 1 {
                        error!(
                            "Ring buffer read error: {}. Buffer state: available={}, last_timestamp={}",
                            e,
                            shared_buffer.available_messages(),
                            shared_buffer.get_last_timestamp()
                        );
                    }

                    if consecutive_errors > 10 {
                        warn!("Too many consecutive errors, resetting read index");
                        shared_buffer.reset_read_index();
                        consecutive_errors = 0;
                    }
                }
            }
        }

        frame_count = frame_count.wrapping_add(1);
        thread::sleep(Duration::from_millis(10));
    }

    info!("Shared memory worker thread exiting");
}

/// Initialize logging system
fn initialize_logging(shared_path: &str) -> Result<(), AppError> {
    let now = Local::now();
    let timestamp = now.format("%Y-%m-%d_%H_%M_%S").to_string();

    let file_name = if shared_path.is_empty() {
        "iced_bar".to_string()
    } else {
        std::path::Path::new(shared_path)
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| format!("iced_bar_{}", name))
            .unwrap_or_else(|| "iced_bar".to_string())
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

fn main() -> iced::Result {
    // Parse command line arguments
    let args: Vec<String> = env::args().collect();
    let class_instance = args.get(0).cloned().unwrap_or_default();
    let shared_path = args.get(1).cloned().unwrap_or_default();

    // Initialize logging
    if let Err(e) = initialize_logging(&shared_path) {
        error!("Failed to initialize logging: {}", e);
        std::process::exit(1);
    }

    info!("Starting iced_bar v{}", 1.0);

    // Create communication channels
    let (message_sender, message_receiver) = mpsc::channel::<SharedMessage>();
    let (command_sender, command_receiver) = mpsc::channel::<SharedCommand>();
    let (heartbeat_sender, heartbeat_receiver) = mpsc::channel();

    let shared_path_clone = shared_path.clone();
    thread::spawn(move || {
        shared_memory_worker(
            shared_path_clone,
            message_sender,
            heartbeat_sender,
            command_receiver,
        )
    });

    // Start heartbeat monitor
    thread::spawn(move || heartbeat_monitor(heartbeat_receiver));

    // 创建应用实例并传入通道
    let app = TabBarExample::new().with_channels(message_receiver, command_sender);

    // 使用 iced::application 的 Builder 模式
    iced::application("iced_bar", TabBarExample::update, TabBarExample::view)
        .window(window::Settings {
            platform_specific: window::settings::PlatformSpecific {
                application_id: class_instance,
                ..Default::default()
            },
            ..Default::default()
        })
        .antialiasing(true)
        .font(NERD_FONT_BYTES)
        .window_size(Size::from([800., 40.]))
        .subscription(TabBarExample::subscription)
        .style(TabBarExample::style)
        // .transparent(true)
        .theme(TabBarExample::theme)
        .run_with(|| (app, iced::Task::none()))
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
enum Message {
    TabSelected(usize),
    LayoutClicked(u32),
    CheckSharedMessages,
    SharedMessageReceived(SharedMessage),
    GetWindowId,
    GetWindowSize(Size),
    GetScaleFactor(f32),
    WindowIdReceived(Option<Id>),
    ResizeWindow,
    ResizeWithId(Option<Id>),
    ShowSecondsToggle,

    // For mouse_area
    MouseEnter,
    MouseExit,
    MouseMove(iced::Point),
    LeftClick,
    RightClick,
}

#[derive(Debug)]
struct TabBarExample {
    active_tab: usize,
    tabs: Vec<String>,
    tab_colors: Vec<Color>,
    // 添加通信通道
    message_receiver: Option<mpsc::Receiver<SharedMessage>>,
    command_sender: Option<mpsc::Sender<SharedCommand>>,
    // 新增：用于显示共享消息的状态
    last_shared_message: Option<SharedMessage>,
    message_count: u32,
    layout_symbol: String,
    monitor_num: u8,
    now: chrono::DateTime<chrono::Local>,
    formated_now: String,
    current_window_id: Option<Id>,
    is_resized: bool,
    scale_factor: f32,
    is_hovered: bool,
    mouse_position: Option<iced::Point>,
    show_seconds: bool,

    /// Audio system
    pub audio_manager: AudioManager,

    /// System monitoring
    pub system_monitor: SystemMonitor,
    transparent: bool,

    cpu_pie: Cache,
    use_circle: bool,
}

impl Default for TabBarExample {
    fn default() -> Self {
        TabBarExample::new()
    }
}

impl TabBarExample {
    const DEFAULT_COLOR: Color = color!(0x666666);
    const TAB_WIDTH: f32 = 32.0;
    const TAB_HEIGHT: f32 = 32.0;
    const TAB_SPACING: f32 = 1.0;
    const UNDERLINE_WIDTH: f32 = 28.0;
    const TEXT_SIZE: f32 = 18.0;

    fn new() -> Self {
        Self {
            active_tab: 0,
            tabs: vec![
                "🍜".to_string(),
                "🎨".to_string(),
                "🍀".to_string(),
                "🧿".to_string(),
                "🌟".to_string(),
                "🐐".to_string(),
                "🏆".to_string(),
                "🕊️".to_string(),
                "🏡".to_string(),
            ],
            tab_colors: vec![
                color!(0xFF6B6B), // 红色
                color!(0x4ECDC4), // 青色
                color!(0x45B7D1), // 蓝色
                color!(0x96CEB4), // 绿色
                color!(0xFECA57), // 黄色
                color!(0xFF9FF3), // 粉色
                color!(0x54A0FF), // 淡蓝色
                color!(0x5F27CD), // 紫色
                color!(0x00D2D3), // 青绿色
            ],
            message_receiver: None,
            command_sender: None,
            last_shared_message: None,
            message_count: 0,
            layout_symbol: String::from(" ? "),
            monitor_num: 0,
            now: chrono::offset::Local::now(),
            formated_now: String::new(),
            current_window_id: None,
            is_resized: false,
            scale_factor: 1.0,
            is_hovered: false,
            mouse_position: None,
            show_seconds: false,
            audio_manager: AudioManager::new(),
            system_monitor: SystemMonitor::new(10),
            transparent: true,
            cpu_pie: Cache::default(),
            use_circle: false,
        }
    }

    // 添加设置通道的方法
    fn with_channels(
        mut self,
        message_receiver: mpsc::Receiver<SharedMessage>,
        command_sender: mpsc::Sender<SharedCommand>,
    ) -> Self {
        self.message_receiver = Some(message_receiver);
        self.command_sender = Some(command_sender);
        self
    }

    fn send_tag_command(&self, command_sender: &mpsc::Sender<SharedCommand>, is_view: bool) {
        if let Some(ref message) = self.last_shared_message {
            let monitor_id = message.monitor_info.monitor_num;

            let tag_bit = 1 << self.active_tab;
            let command = if is_view {
                SharedCommand::view_tag(tag_bit, monitor_id)
            } else {
                SharedCommand::toggle_tag(tag_bit, monitor_id)
            };

            match command_sender.send(command) {
                Ok(_) => {
                    let action = if is_view { "ViewTag" } else { "ToggleTag" };
                    log::info!(
                        "Sent {} command for tag {} in channel",
                        action,
                        self.active_tab + 1
                    );
                }
                Err(e) => {
                    let action = if is_view { "ViewTag" } else { "ToggleTag" };
                    log::error!("Failed to send {} command: {}", action, e);
                }
            }
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TabSelected(index) => {
                info!("Tab selected: {}", index);
                self.active_tab = index;

                // 发送命令到共享内存
                if let Some(ref command_sender) = self.command_sender {
                    self.send_tag_command(command_sender, true);
                }

                Task::none()
            }

            Message::LayoutClicked(layout_index) => {
                if let Some(ref message) = self.last_shared_message {
                    let monitor_id = message.monitor_info.monitor_num;
                    let command =
                        SharedCommand::new(CommandType::SetLayout, layout_index, monitor_id);
                    if let Some(ref command_sender) = self.command_sender {
                        self.send_tag_command(command_sender, true);
                        if let Err(e) = command_sender.send(command) {
                            log::error!("Failed to send SetLayout command: {}", e);
                        } else {
                            log::info!("Sent SetLayout command: layout_index={}", layout_index);
                        }
                    }
                }
                Task::none()
            }

            Message::GetWindowId => {
                info!("GetWindowId");
                // 获取最新窗口的 ID
                window::get_latest().map(Message::WindowIdReceived)
            }

            Message::MouseEnter => {
                self.is_hovered = true;
                // info!("鼠标进入区域");
                Task::none()
            }

            Message::ShowSecondsToggle => {
                self.show_seconds = !self.show_seconds;
                Task::none()
            }

            Message::MouseExit => {
                self.is_hovered = false;
                self.mouse_position = None;
                // info!("鼠标离开区域");
                Task::none()
            }

            Message::MouseMove(point) => {
                self.mouse_position = Some(point);
                // info!("鼠标位置: ({:.1}, {:.1})", point.x, point.y);
                Task::none()
            }

            Message::LeftClick => {
                // info!("左键点击");
                let _ = Command::new("flameshot").arg("gui").spawn();
                Task::none()
            }

            Message::RightClick => {
                // info!("右键点击");
                Task::none()
            }

            Message::GetWindowSize(window_size) => {
                info!("window_size: {:?}", window_size);
                Task::none()
            }

            Message::GetScaleFactor(scale_factor) => {
                info!("scale_factor: {}", scale_factor);
                self.scale_factor = scale_factor;
                Task::none()
            }

            Message::WindowIdReceived(window_id) => {
                info!("WindowIdReceived");
                // 保存窗口 ID 并用于后续操作
                self.current_window_id = window_id;
                info!("current_window_id: {:?}", self.current_window_id);
                Task::batch([
                    window::get_size(window_id.unwrap()).map(Message::GetWindowSize),
                    window::get_scale_factor(window_id.unwrap()).map(Message::GetScaleFactor),
                ])
            }

            Message::ResizeWindow => {
                info!("ResizeWindow");
                if let Some(id) = self.current_window_id {
                    Task::perform(async move { Some(id) }, Message::ResizeWithId)
                } else {
                    window::get_latest().map(|id| Message::ResizeWithId(id))
                }
            }

            Message::ResizeWithId(window_id) => {
                info!("ResizeWithId");
                self.current_window_id = window_id;
                if let Some(id) = self.current_window_id {
                    let mut tasks = Vec::new();
                    if let Some(ref shared_msg) = self.last_shared_message {
                        let monitor_info = &shared_msg.monitor_info;
                        let width = (monitor_info.monitor_width as f32
                            - 2.0 * monitor_info.border_w as f32)
                            / self.scale_factor;
                        let height = 40.0;
                        let window_pos = Point::new(
                            (monitor_info.monitor_x as f32 + monitor_info.border_w as f32)
                                / self.scale_factor,
                            (monitor_info.monitor_y as f32 + monitor_info.border_w as f32 * 0.5)
                                / self.scale_factor,
                        );
                        let window_size = Size::new(width, height);
                        info!("window_pos: {:?}", window_pos);
                        info!("window_size: {:?}", window_size);
                        tasks.push(window::move_to(id, window_pos));
                        tasks.push(window::resize(id, window_size));
                        self.is_resized = true;
                    }
                    Task::batch(tasks)
                } else {
                    window::get_latest().map(|id| Message::ResizeWithId(id))
                }
            }

            Message::CheckSharedMessages => {
                // info!("CheckSharedMessages");
                let now = Local::now();
                let format_str = if self.show_seconds {
                    "%Y-%m-%d %H:%M:%S"
                // "%Y-%m-%d_%H_%M_%S"
                } else {
                    "%Y-%m-%d %H:%M"
                };
                if now.timestamp() != self.now.timestamp() {
                    self.now = now;
                    self.formated_now = now.format(format_str).to_string();
                    // info!("formated_now: {}", self.formated_now);
                }

                // Update system monitor
                self.system_monitor.update_if_needed();

                // Update audio manager
                self.audio_manager.update_if_needed();

                let mut tasks = Vec::new();
                START.call_once(|| {
                    // run initialization here
                    if self.current_window_id.is_none() {
                        tasks.push(Task::perform(async {}, |_| Message::GetWindowId));
                    }
                });
                if self.current_window_id.is_none() {
                    // Wait for current_window_id update.
                    return Task::batch(tasks);
                }
                let current_window_id = self.current_window_id;
                if !self.is_resized {
                    tasks.push(Task::perform(
                        async move { current_window_id },
                        Message::ResizeWithId,
                    ));
                }

                if let Some(ref receiver) = self.message_receiver {
                    // 非阻塞地读取所有可用消息
                    while let Ok(shared_msg) = receiver.try_recv() {
                        // info!("recieve shared_msg: {:?}", shared_msg);
                        tasks.push(Task::perform(
                            async move { shared_msg },
                            Message::SharedMessageReceived,
                        ));
                    }
                }

                Task::batch(tasks)
            }

            Message::SharedMessageReceived(shared_msg) => {
                info!("SharedMessageReceived");
                info!("recieve shared_msg: {:?}", shared_msg);
                // 更新应用状态
                self.last_shared_message = Some(shared_msg.clone());
                self.message_count += 1;
                self.layout_symbol = shared_msg.monitor_info.ltsymbol;
                self.monitor_num = shared_msg.monitor_info.monitor_num as u8;
                for (index, tag_status) in shared_msg.monitor_info.tag_status_vec.iter().enumerate()
                {
                    if tag_status.is_selected {
                        self.active_tab = index;
                    }
                }
                Task::none()
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        time::every(Duration::from_millis(50)).map(|_| Message::CheckSharedMessages)
    }

    fn style(&self, theme: &Theme) -> Appearance {
        if self.transparent {
            Appearance {
                background_color: Color::TRANSPARENT,
                text_color: theme.palette().text,
            }
        } else {
            Appearance {
                background_color: theme.palette().background,
                text_color: theme.palette().text,
            }
        }
    }

    fn theme(&self) -> Theme {
        Theme::Dracula
        // Theme::ALL[(self.now.timestamp() as usize / 10) % Theme::ALL.len()].clone()
    }

    fn monitor_num_to_icon(monitor_num: u8) -> &'static str {
        match monitor_num {
            0 => "🥇",
            1 => "🥈",
            2 => "🥉",
            _ => "?",
        }
    }

    fn view_work_space(&self) -> Element<Message> {
        let tab_bar = self
            .tabs
            .iter()
            .fold(TabBar::new(Message::TabSelected), |tab_bar, tab_label| {
                let idx = tab_bar.size();
                tab_bar.push(idx, TabLabel::Text(tab_label.to_owned()))
            })
            .set_active_tab(&self.active_tab)
            .tab_width(Length::Fixed(Self::TAB_WIDTH))
            .height(Length::Fixed(Self::TAB_HEIGHT))
            .spacing(Self::TAB_SPACING)
            .padding(0.0)
            .width(Length::Shrink)
            .text_size(Self::TEXT_SIZE);

        let layout_text =
            container(rich_text([span(self.layout_symbol.clone())])).center_x(Length::Shrink);

        let scrollable_content = Scrollable::with_direction(
            row![
                button("[]=").on_press(Message::LayoutClicked(0)),
                button("><>").on_press(Message::LayoutClicked(1)),
                button("[M]").on_press(Message::LayoutClicked(2)),
            ]
            .spacing(10)
            .padding(0.0),
            Direction::Horizontal(Scrollbar::new().scroller_width(3.0).width(1.)),
        )
        .width(50.0)
        .height(Self::TAB_HEIGHT)
        .spacing(0.);

        let cyan = Color::from_rgb(0.0, 1.0, 1.0); // 青色
        let dark_orange = Color::from_rgb(1.0, 0.5, 0.0); // 深橙色

        let screenshot_text = container(text(format!(" s {:.2} ", self.scale_factor)).center())
            .center_y(Length::Fill)
            .style(move |_theme: &Theme| {
                if self.is_hovered {
                    container::Style {
                        text_color: Some(dark_orange),
                        border: Border {
                            radius: border::radius(2.0),
                            ..Default::default()
                        },
                        background: Some(Background::Color(cyan)),
                        ..Default::default()
                    }
                } else {
                    container::Style {
                        border: Border {
                            color: Color::WHITE,
                            width: 0.5,
                            radius: border::radius(2.0),
                        },
                        ..Default::default()
                    }
                }
            })
            .padding(0.0);
        let time_button = button(self.formated_now.as_str()).on_press(Message::ShowSecondsToggle);
        let canvas = canvas(self as &Self)
            .width(Length::Fixed(Self::TAB_HEIGHT))
            .height(Length::Fixed(Self::TAB_HEIGHT));

        let work_space_row = Row::new()
            .push(tab_bar)
            .push(Space::with_width(3))
            .push(layout_text)
            .push(Space::with_width(3))
            .push(scrollable_content)
            .push(Space::with_width(Length::Fill))
            .push(container(canvas).style(move |_theme| {
                let gradient = gradient::Linear::new(0.0)
                    .add_stop(0.0, Color::from_rgb(0.0, 1.0, 1.0))
                    .add_stop(1.0, Color::from_rgb(1.0, 0., 0.));
                gradient.into()
            }))
            .push(Space::with_width(3))
            .push(
                mouse_area(screenshot_text)
                    .on_enter(Message::MouseEnter)
                    .on_exit(Message::MouseExit)
                    .on_press(Message::LeftClick),
            )
            .push(Space::with_width(3))
            .push(time_button)
            .push(rich_text([
                span(" "),
                span(Self::monitor_num_to_icon(self.monitor_num)),
            ]))
            .align_y(iced::Alignment::Center);

        work_space_row.into()
    }

    fn view_under_line(&self) -> Element<Message> {
        // 创建下划线行
        let mut tag_status_vec = Vec::new();
        if let Some(ref shared_msg) = self.last_shared_message {
            tag_status_vec = shared_msg.monitor_info.tag_status_vec.clone();
        }

        let mut underline_row = Row::new().spacing(Self::TAB_SPACING);
        for (index, _) in self.tabs.iter().enumerate() {
            // 创建下划线
            let tab_color = self.tab_colors.get(index).unwrap_or(&Self::DEFAULT_COLOR);

            // 根据状态设置样式
            if let Some(tag_status) = tag_status_vec.get(index) {
                if !(tag_status.is_selected
                    || tag_status.is_occ
                    || tag_status.is_filled
                    || tag_status.is_urg)
                {
                    let underline = container(text(" "))
                        .width(Length::Fixed(Self::TAB_WIDTH))
                        .height(Length::Fixed(3.0));
                    underline_row = underline_row.push(underline);
                    continue;
                }
                if tag_status.is_urg {
                    let underline = container(
                        container(text(" "))
                            .width(Length::Fixed(Self::UNDERLINE_WIDTH))
                            .height(Length::Fixed(4.0))
                            .style(move |_theme: &Theme| container::Style {
                                background: Some(Background::Color(Color::from_rgb(1., 0., 0.))),
                                ..Default::default()
                            }),
                    )
                    .center_x(Length::Fixed(Self::TAB_WIDTH));
                    underline_row = underline_row.push(underline);
                    continue;
                }
                if tag_status.is_filled {
                    let underline = container(
                        container(text(" "))
                            .width(Length::Fixed(Self::UNDERLINE_WIDTH))
                            .height(Length::Fixed(4.0))
                            .style(move |_theme: &Theme| container::Style {
                                background: Some(Background::Color(Color::from_rgb(0., 1., 0.))),
                                ..Default::default()
                            }),
                    )
                    .center_x(Length::Fixed(Self::TAB_WIDTH));
                    underline_row = underline_row.push(underline);
                    continue;
                }
                if tag_status.is_selected && !tag_status.is_occ {
                    let underline = container(
                        container(text(" "))
                            .width(Length::Fixed(Self::UNDERLINE_WIDTH))
                            .height(Length::Fixed(3.0))
                            .style(move |_theme: &Theme| container::Style {
                                background: Some(Background::Color(Self::DEFAULT_COLOR)),
                                ..Default::default()
                            }),
                    )
                    .center_x(Length::Fixed(Self::TAB_WIDTH));
                    underline_row = underline_row.push(underline);
                    continue;
                }
                if !tag_status.is_selected && tag_status.is_occ {
                    let underline = container(
                        container(text(" "))
                            .width(Length::Fixed(Self::UNDERLINE_WIDTH))
                            .height(Length::Fixed(1.0))
                            .style(move |_theme: &Theme| container::Style {
                                background: Some(Background::Color(*tab_color)),
                                ..Default::default()
                            }),
                    )
                    .center_x(Length::Fixed(Self::TAB_WIDTH));
                    underline_row = underline_row.push(underline);
                    continue;
                }
                if tag_status.is_selected && tag_status.is_occ {
                    let underline = container(
                        container(text(" "))
                            .width(Length::Fixed(Self::UNDERLINE_WIDTH))
                            .height(Length::Fixed(3.0))
                            .style(move |_theme: &Theme| container::Style {
                                background: Some(Background::Color(*tab_color)),
                                ..Default::default()
                            }),
                    )
                    .center_x(Length::Fixed(Self::TAB_WIDTH));
                    underline_row = underline_row.push(underline);
                    continue;
                }
            } else {
                // Use default logic.
                let is_active = index == self.active_tab;
                let underline = if is_active {
                    // 激活状态：显示彩色下划线
                    container(
                        container(text(" "))
                            .width(Length::Fixed(Self::UNDERLINE_WIDTH))
                            .height(Length::Fixed(3.0))
                            .style(move |_theme: &Theme| container::Style {
                                background: Some(Background::Color(*tab_color)),
                                ..Default::default()
                            }),
                    )
                    .center_x(Length::Fixed(Self::TAB_WIDTH))
                } else {
                    // 非激活状态：透明占位符
                    container(text(" "))
                        .width(Length::Fixed(Self::TAB_WIDTH))
                        .height(Length::Fixed(3.0))
                };
                underline_row = underline_row.push(underline);
            }
        }
        let (memory_available, memory_used) =
            if let Some(snapshot) = self.system_monitor.get_snapshot() {
                (
                    snapshot.memory_available as f32 / 1e9, // GB
                    snapshot.memory_used as f32 / 1e9,      // GB
                )
            } else {
                (0.0, 0.0)
            };
        underline_row = underline_row.push(Space::with_width(Length::Fill)).push(
            progress_bar(
                0.0..=100.0,
                memory_available / (memory_available + memory_used) * 100.0,
            )
            .style(move |theme: &Theme| progress_bar::Style {
                background: Background::Gradient({
                    let gradient = gradient::Linear::new(Radians::from(Degrees(-90.0)))
                        .add_stop(0.0, Color::from_rgb(0.0, 1.0, 1.0))
                        .add_stop(1.0, Color::from_rgb(1.0, 0., 0.));
                    gradient.into()
                }),
                bar: Background::Color(theme.palette().primary),
                border: border::rounded(2.0),
            })
            .height(Length::Fixed(3.0))
            .width(200.),
        );
        underline_row.into()
    }

    fn view(&self) -> Element<Message> {
        // let work_space_row = self.view_work_space().explain(Color::from_rgb(1., 0., 1.));
        let work_space_row = self.view_work_space();

        let under_line_row = self.view_under_line();

        Column::new()
            .padding(2)
            .spacing(2)
            .push(work_space_row)
            .push(under_line_row)
            .into()
    }
}

impl<Message> canvas::Program<Message> for TabBarExample {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let cpu_average = if let Some(snapshot) = self.system_monitor.get_snapshot() {
            snapshot.cpu_average
        } else {
            0.0
        };
        self.cpu_pie.clear();
        let sector = if self.use_circle {
            self.cpu_pie.draw(renderer, bounds.size(), |frame| {
                // 1. 定义扇形的几何属性
                let center = frame.center();
                let radius = frame.width().min(frame.height()) * 0.5;
                let palette = theme.extended_palette();
                let background = Path::circle(center, radius);
                frame.fill(&background, palette.secondary.strong.color);

                let start_angle_deg = 0.0;
                let end_angle_deg = -360.0 * cpu_average / 100.0;
                // 将角度转换为弧度
                let start_angle_rad = Radians::from(Degrees(start_angle_deg));
                let end_angle_rad = Radians::from(Degrees(end_angle_deg));
                let sector_path = Path::new(|builder| {
                    // 步骤 a: 将画笔移动到圆心
                    builder.move_to(center);
                    // 步骤 b: 画第一条半径。我们需要计算出圆弧的起点坐标，然后画一条直线过去。
                    let start_point = Point {
                        x: center.x + radius * start_angle_rad.0.cos(),
                        y: center.y + radius * start_angle_rad.0.sin(),
                    };
                    builder.line_to(start_point);
                    // 步骤 c: 绘制圆弧。此时画笔位于圆弧起点，arc会从这个点开始画。
                    builder.arc(Arc {
                        center,
                        radius,
                        start_angle: start_angle_rad.into(),
                        end_angle: end_angle_rad.into(),
                    });
                    // 步骤 d: 闭合路径。这会从圆弧的终点画一条直线回到整个路径的起点（即圆心），
                    builder.line_to(center);
                });

                // 3. 填充路径
                let fill_color = Color::from_rgb8(0, 150, 255);
                frame.fill(&sector_path, fill_color);
            })
        } else {
            self.cpu_pie.draw(renderer, bounds.size(), |frame| {
                let width = bounds.width;
                let height = bounds.height;
                let used_height = height * (1. - cpu_average / 100.0);
                frame.fill_rectangle(Point::ORIGIN, Size::new(width, used_height), Color::BLACK);
            })
        };

        vec![sector]
    }
}
