use chrono::Local;
use flexi_logger::{Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming};
use iced::time::{self};
use iced::widget::{Space, button, rich_text};
use iced::window::Id;
use iced::{
    Background, Border, Color, Element, Length, Padding, Subscription, Task, Theme, color,
    widget::{Column, Row, container, text},
};
use iced::{Settings, Size, window};
mod error;
pub use error::AppError;
use iced_aw::{TabBar, TabLabel};
use iced_fonts::NERD_FONT_BYTES;
use log::{error, info, warn};
use shared_structures::{SharedCommand, SharedMessage, SharedRingBuffer};
use std::env;
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
        Path::new(shared_path)
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
        .font(NERD_FONT_BYTES)
        .window_size(Size::from([800., 40.]))
        .scale_factor(TabBarExample::scale_factor)
        .subscription(TabBarExample::subscription)
        .theme(TabBarExample::theme)
        .run_with(|| (app, iced::Task::none()))
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
enum Message {
    TabSelected(usize),
    LayoutClicked,
    CheckSharedMessages,
    SharedMessageReceived(SharedMessage),
    GetWindowId,
    GetWindowSize(Size),
    GetScaleFactor(f32),
    WindowIdReceived(Option<Id>),
    ResizeWindow,
    ResizeWithId(Option<Id>),
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

    now: chrono::DateTime<chrono::Local>,
    current_window_id: Option<Id>,
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

            now: chrono::offset::Local::now(),
            current_window_id: None,
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

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TabSelected(index) => {
                info!("Tab selected: {}", index);
                self.active_tab = index;

                // 发送命令到共享内存
                if let Some(ref _sender) = self.command_sender {
                    // let cmd = SharedCommand::TabChanged(index); // 假设有这个命令
                    // if let Err(e) = sender.send(cmd) {
                    //     error!("Failed to send command: {}", e);
                    // }
                }

                Task::none()
            }

            Message::LayoutClicked => Task::none(),

            Message::GetWindowId => {
                info!("GetWindowId");
                // 获取最新窗口的 ID
                window::get_latest().map(Message::WindowIdReceived)
            }

            Message::GetWindowSize(window_size) => {
                info!("window_size: {:?}", window_size);
                Task::none()
            }

            Message::GetScaleFactor(scale_factor) => {
                info!("scale_factor: {}", scale_factor);
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
                    let mut tasks = Vec::new();
                    tasks.push(window::move_to(id, iced::Point::new(0., 0.)));
                    tasks.push(window::resize(id, Size::new(800.0, 40.0)));
                    Task::batch(tasks)
                } else {
                    window::get_latest().map(|id| Message::ResizeWithId(id))
                }
            }

            Message::ResizeWithId(window_id) => {
                info!("ResizeWithId");
                self.current_window_id = window_id;
                if let Some(id) = self.current_window_id {
                    let mut tasks = Vec::new();
                    tasks.push(window::move_to(id, iced::Point::new(0., 0.)));
                    tasks.push(window::resize(id, Size::new(1000., 40.0)));
                    Task::batch(tasks)
                } else {
                    window::get_latest().map(|id| Message::ResizeWithId(id))
                }
            }

            Message::CheckSharedMessages => {
                // info!("CheckSharedMessages");
                let now = Local::now();
                if now != self.now {
                    self.now = now;
                    // let timestamp = now.format("%Y-%m-%d_%H_%M_%S").to_string();
                    // info!("timestamp: {}", timestamp);
                }
                // 检查并处理所有待处理的消息
                let mut tasks = Vec::new();

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

                if self.current_window_id.is_none() {
                    tasks.push(Task::perform(async {}, |_| Message::GetWindowId));
                } else {
                    // let current_window_id = self.current_window_id;
                    // tasks.push(
                    //     window::get_size(current_window_id.unwrap()).map(Message::GetWindowSize),
                    // );
                    // tasks.push(
                    //     window::get_scale_factor(current_window_id.unwrap())
                    //         .map(Message::GetScaleFactor),
                    // );
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
                let current_window_id = self.current_window_id;
                Task::perform(async move { current_window_id }, Message::ResizeWithId)
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        time::every(Duration::from_millis(50)).map(|_| Message::CheckSharedMessages)
    }

    fn scale_factor(&self) -> f64 {
        1.0
    }

    fn theme(&self) -> Theme {
        Theme::ALL[(self.now.timestamp() as usize / 10) % Theme::ALL.len()].clone()
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
            .padding(1.0)
            .width(Length::Shrink)
            .text_size(Self::TEXT_SIZE);

        let layout_text = container(text(self.layout_symbol.as_str()).size(Self::TEXT_SIZE))
            .center_x(Length::Shrink);

        let work_space_row = Row::new()
            .push(tab_bar)
            .push(layout_text)
            .push(Space::with_width(Length::Fill))
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
                                border: Border::default(),
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
                                border: Border::default(),
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
                                border: Border::default(),
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
                                border: Border::default(),
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
                                border: Border::default(),
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
                                border: Border::default(),
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
        underline_row.into()
    }

    fn view(&self) -> Element<Message> {
        // let work_space_row = self.view_work_space().explain(Color::from_rgb(1., 0., 1.));
        let work_space_row = self.view_work_space();

        let under_line_row = self.view_under_line();

        Column::new()
            .push(work_space_row)
            .push(under_line_row)
            .spacing(2)
            .padding(2)
            .height(Length::Fixed(50.))
            .into()
    }
}
