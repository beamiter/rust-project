use chrono::Local;
use flexi_logger::{Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming};
use iced::futures::channel::mpsc::{self};
use iced::futures::{SinkExt, Stream};
use iced::time::{self, milliseconds};
use iced::widget::container;
use iced::widget::lazy;
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{Scrollable, Space, button, progress_bar, rich_text, row};
use iced::widget::{mouse_area, span};
use iced::{gradient, stream, theme};

use iced::window::Id;
use iced::{
    Background, Border, Color, Degrees, Element, Length, Point, Radians, Size, Subscription, Task,
    Theme, border, color,
    widget::{Column, Row, text},
    window,
};
mod error;
pub use error::AppError;
use log::debug;
use log::{error, info, warn};
use shared_structures::{CommandType, MonitorInfo, SharedCommand, SharedMessage, SharedRingBuffer};
use std::env;
use std::process::Command;
use std::sync::Once;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::audio_manager::AudioManager;
use crate::system_monitor::SystemMonitor;

pub mod audio_manager;
pub mod system_monitor;

static _START: Once = Once::new();

#[allow(unused_macros)]
macro_rules! create_tab_button {
    ($self:expr, $index:expr) => {
        lazy(&$self.active_tab, |_| {
            mouse_area(
                button(
                    rich_text![span($self.tabs[$index].clone())]
                        .on_link_click(std::convert::identity),
                )
                .width(Self::TAB_WIDTH),
            )
            .on_press(Message::TabSelected($index))
        })
    };
}
macro_rules! tab_buttons {
    (
        $self:expr;
        $($name:ident[$index:expr]),* $(,)?
    ) => {
        $(
            let $name = lazy(&$self.active_tab, |_| {
                mouse_area(
                    button(
                        rich_text![span($self.tabs[$index].clone())].on_link_click(std::convert::identity),
                    )
                    .width(Self::TAB_WIDTH),
                )
                .on_press(Message::TabSelected($index))
            });
        )*
    };

    // 支持自定义属性
    (
        $self:expr;
        $(
            $(#[$attr:meta])*
            $name:ident[$index:expr]
        ),* $(,)?
    ) => {
        $(
            $(#[$attr])*
            let $name = lazy(&$self.active_tab, |_| {
                mouse_area(
                    button(
                        rich_text![span($self.tabs[$index].clone())].on_link_click(std::convert::identity),
                    )
                    .width(Self::TAB_WIDTH),
                )
                .on_press(Message::TabSelected($index))
            });
        )*
    };
}

/// Initialize logging system
fn initialize_logging(shared_path: &str) -> Result<(), AppError> {
    let tmp_now = Local::now();
    let timestamp = tmp_now.format("%Y-%m-%d_%H_%M_%S").to_string();

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
                .directory("/tmp/jwm")
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
    let args: Vec<String> = env::args().collect();
    let application_id = args.get(0).cloned().unwrap_or_default();
    info!("application_id: {application_id}");
    iced::application(IcedBar::new, IcedBar::update, IcedBar::view)
        .window(window::Settings {
            platform_specific: window::settings::PlatformSpecific {
                application_id,
                ..Default::default()
            },
            ..Default::default()
        })
        // .font(include_bytes!("../fonts/NotoColorEmoji.ttf").as_slice())
        .window_size(Size::from([800., 40.]))
        .subscription(IcedBar::subscription)
        .title("iced_bar")
        .scale_factor(IcedBar::scale_factor)
        // .style(IcedBar::style)
        .theme(|_| Theme::Light)
        .run()
}

#[allow(dead_code)]
enum Input {
    DoSomeWork,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
enum Message {
    TabSelected(usize),
    LayoutClicked(u32),
    ShowSecondsToggle,

    GetWindowId,
    WindowIdReceived(Option<Id>),

    GetAndResizeWindowSize(Size),

    GetScaleFactor(f32),
    RawIdReceived(u64),

    MouseEnterScreenShot,
    MouseExitScreenShot,
    LeftClick,
    RightClick,

    UpdateTime,
    SharedMemoryUpdated(SharedMessage),
    SharedMemoryError(String),
}

struct IcedBar {
    active_tab: usize,
    tabs: [String; 9],
    tab_colors: [Color; 9],
    shared_buffer_opt: Option<SharedRingBuffer>,
    shared_path: String,
    monitor_info_opt: Option<MonitorInfo>,
    formated_now: String,
    heartbeat_timestamp: AtomicI64,
    raw_window_id: AtomicU64,
    current_window_id: Option<Id>,
    scale_factor: f32,
    is_hovered: bool,
    mouse_position: Option<iced::Point>,
    show_seconds: bool,
    layout_symbol: String,
    monitor_num: i32,
    current_window_size: Option<Size>,
    target_window_pos: Option<Point>,
    target_window_size: Option<Size>,

    /// Audio system
    audio_manager: AudioManager,

    /// System monitoring
    system_monitor: SystemMonitor,
    transparent: bool,
}

impl Default for IcedBar {
    fn default() -> Self {
        IcedBar::new()
    }
}

impl IcedBar {
    const DEFAULT_COLOR: Color = color!(0x666666);
    const TAB_WIDTH: f32 = 40.0;
    const TAB_HEIGHT: f32 = 32.0;
    const TAB_SPACING: f32 = 2.0;
    const UNDERLINE_WIDTH: f32 = 28.0;

    fn new() -> Self {
        // Parse command line arguments
        let args: Vec<String> = env::args().collect();
        let shared_path = args.get(1).cloned().unwrap_or_default();
        // Initialize logging
        if let Err(e) = initialize_logging(&shared_path) {
            error!("Failed to initialize logging: {}", e);
            std::process::exit(1);
        }
        info!("Starting iced_bar v{}, shared_path: {shared_path}", 1.0);

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
        let heartbeat_timestamp = AtomicI64::new(Local::now().timestamp());
        let raw_window_id = AtomicU64::new(0);

        Self {
            active_tab: 0,
            tabs: [
                "🍜".to_string(),
                "🎨".to_string(),
                "🍀".to_string(),
                "🧿".to_string(),
                "🌟".to_string(),
                "🐐".to_string(),
                "🐢".to_string(),
                "🦣".to_string(),
                "🏡".to_string(),
            ],
            tab_colors: [
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
            shared_buffer_opt,
            shared_path,
            monitor_info_opt: None,
            formated_now: String::new(),
            current_window_id: None,
            scale_factor: 1.0,
            is_hovered: false,
            mouse_position: None,
            show_seconds: false,
            layout_symbol: String::new(),
            monitor_num: 0,
            current_window_size: None,
            target_window_pos: None,
            target_window_size: None,
            audio_manager: AudioManager::new(),
            system_monitor: SystemMonitor::new(10),
            transparent: true,
            heartbeat_timestamp,
            raw_window_id,
        }
    }

    fn prepare_worker() -> impl Stream<Item = Message> {
        stream::channel(10, async |mut output| {
            let _ = output.send(Message::GetWindowId).await;
        })
    }

    fn message_notify_subscription(shared_path: String) -> Subscription<Message> {
        Subscription::run_with(shared_path.clone(), move |path| {
            let path = path.clone();
            stream::channel(100, move |mut output: mpsc::Sender<Message>| {
                let path = path.clone();
                async move {
                    if path.is_empty() {
                        let _ = output
                            .send(Message::SharedMemoryError("Empty shared path".to_string()))
                            .await;
                        return;
                    }
                    // 使用 spawn_blocking 来处理阻塞操作
                    let shared_buffer = match SharedRingBuffer::open(&path, None) {
                        Ok(buffer) => buffer,
                        Err(e) => {
                            let _ = output
                                .send(Message::SharedMemoryError(format!(
                                    "Failed to open shared buffer: {}",
                                    e
                                )))
                                .await;
                            return;
                        }
                    };

                    let shared_buffer = std::sync::Arc::new(shared_buffer);
                    let buffer_clone = shared_buffer.clone();

                    let (tx, mut rx) = tokio::sync::mpsc::channel(100);

                    tokio::task::spawn_blocking(move || {
                        let mut prev_timestamp = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap()
                            .as_millis();
                        loop {
                            match buffer_clone.wait_for_message(Some(time::Duration::from_secs(2)))
                            {
                                Ok(true) => {
                                    if let Ok(Some(message)) =
                                        buffer_clone.try_read_latest_message()
                                    {
                                        if prev_timestamp != message.timestamp.into() {
                                            prev_timestamp = message.timestamp.into();
                                            debug!(
                                                "[notifier] Received State: {}",
                                                message.timestamp
                                            );
                                            let _ = tx
                                                .blocking_send(Message::SharedMemoryUpdated(
                                                    message,
                                                ))
                                                .unwrap();
                                        }
                                    }
                                }
                                Ok(false) => debug!("[notifier] Wait for message timed out."),
                                Err(e) => {
                                    error!("[notifier] Wait for message failed: {}", e);
                                    break;
                                }
                            }
                        }
                    });
                    while let Some(msg) = rx.recv().await {
                        let _ = output.send(msg).await;
                    }
                }
            })
        })
    }

    fn send_tag_command(&mut self, is_view: bool) {
        let tag_bit = 1 << self.active_tab;
        let command = if is_view {
            SharedCommand::view_tag(tag_bit, self.monitor_num)
        } else {
            SharedCommand::toggle_tag(tag_bit, self.monitor_num)
        };

        if let Some(shared_buffer) = &self.shared_buffer_opt {
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

    fn send_layout_command(&mut self, layout_index: u32) {
        let command = SharedCommand::new(CommandType::SetLayout, layout_index, self.monitor_num);
        if let Some(shared_buffer) = &self.shared_buffer_opt {
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

    fn update(&mut self, message: Message) -> Task<Message> {
        // info!("update");
        match message {
            Message::TabSelected(tab_index) => {
                info!("Tab selected: {}", tab_index);
                self.active_tab = tab_index;
                self.send_tag_command(true);

                Task::none()
            }

            Message::LayoutClicked(layout_index) => {
                self.send_layout_command(layout_index);
                info!("Layout selected: {}", layout_index);

                Task::none()
            }

            Message::GetWindowId => {
                info!("GetWindowId");
                window::get_latest().map(Message::WindowIdReceived)
            }

            Message::WindowIdReceived(window_id) => {
                info!("WindowIdReceived");
                self.current_window_id = window_id;
                info!("current_window_id: {:?}", self.current_window_id);
                let window_id = window_id.unwrap();
                Task::batch([
                    window::get_scale_factor(window_id).map(Message::GetScaleFactor),
                    window::get_raw_id::<Message>(window_id).map(Message::RawIdReceived),
                ])
            }

            Message::MouseEnterScreenShot => {
                self.is_hovered = true;
                Task::none()
            }

            Message::MouseExitScreenShot => {
                self.is_hovered = false;
                self.mouse_position = None;
                Task::none()
            }

            Message::ShowSecondsToggle => {
                self.show_seconds = !self.show_seconds;
                return Task::perform(async {}, |_| Message::UpdateTime);
            }

            Message::LeftClick => {
                let _ = Command::new("flameshot").arg("gui").spawn();
                Task::none()
            }

            Message::RightClick => Task::none(),

            Message::GetAndResizeWindowSize(window_size) => {
                // info!("window_size: {:?}", window_size);
                self.current_window_size = Some(window_size);
                if let (
                    Some(current_window_id),
                    Some(current_window_size),
                    Some(target_window_size),
                    Some(target_window_pos),
                ) = (
                    self.current_window_id,
                    self.current_window_size,
                    self.target_window_size,
                    self.target_window_pos,
                ) {
                    if (current_window_size.width - target_window_size.width).abs() > 10.
                        || (current_window_size.height - target_window_size.height).abs() > 5.
                    {
                        info!(
                            "current_window_size: {:?}, target_window_size: {:?}",
                            self.current_window_size, self.target_window_size
                        );
                        return Task::batch([
                            window::move_to(current_window_id, target_window_pos),
                            window::resize(current_window_id, target_window_size),
                        ]);
                    }
                }

                Task::none()
            }

            Message::GetScaleFactor(scale_factor) => {
                info!("scale_factor: {}", scale_factor);
                self.scale_factor = scale_factor;
                Task::none()
            }

            Message::RawIdReceived(raw_id) => {
                // Use xwininfo to get window id.
                // xdotool windowsize 0xc00004 800 40 work!
                self.raw_window_id.store(raw_id, Ordering::Relaxed);
                info!("{}", format!("RawIdReceived: 0x{:X}", raw_id));
                Task::none()
            }

            Message::UpdateTime => {
                let tmp_now = Local::now();
                let format_str = if self.show_seconds {
                    "%Y-%m-%d %H:%M:%S"
                } else {
                    "%Y-%m-%d %H:%M"
                };
                self.heartbeat_timestamp
                    .store(tmp_now.timestamp(), Ordering::Release);
                self.formated_now = tmp_now.format(format_str).to_string();

                if tmp_now.timestamp() % 2 == 0 {
                    self.system_monitor.update_if_needed();
                    self.audio_manager.update_if_needed();
                }

                Task::none()
            }

            Message::SharedMemoryUpdated(message) => {
                info!("SharedMemoryUpdated: {:?}", message);
                self.monitor_info_opt = Some(message.monitor_info);
                if let Some(monitor_info) = self.monitor_info_opt.as_ref() {
                    self.layout_symbol = monitor_info.get_ltsymbol();
                    self.monitor_num = monitor_info.monitor_num;
                    for (index, tag_status) in monitor_info.tag_status_vec.iter().enumerate() {
                        if tag_status.is_selected {
                            self.active_tab = index;
                        }
                    }
                }
                // let mut tasks = Vec::new();
                // tasks.push(
                //     window::get_size(self.current_window_id.unwrap())
                //         .map(Message::GetAndResizeWindowSize),
                // );
                // Task::batch(tasks)
                Task::none()
            }

            Message::SharedMemoryError(err) => {
                info!("SharedMemoryError: {err}");
                Task::none()
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        let tick = if self.current_window_id.is_none() {
            Subscription::run(Self::prepare_worker)
        } else {
            Subscription::batch(vec![
                time::every(milliseconds(1000)).map(|_| Message::UpdateTime),
                Self::message_notify_subscription(self.shared_path.clone()),
            ])
        };
        return tick;
    }

    #[allow(dead_code)]
    fn style(&self, theme: &Theme) -> theme::Style {
        if self.transparent {
            theme::Style {
                background_color: Color::TRANSPARENT,
                text_color: theme.palette().text,
            }
        } else {
            theme::default(theme)
        }
    }

    fn scale_factor(&self) -> f64 {
        1.0
    }

    fn monitor_num_to_icon(monitor_num: u8) -> &'static str {
        match monitor_num {
            0 => "🥇",
            1 => "🥈",
            2 => "🥉",
            _ => "?",
        }
    }

    fn view_work_space(&self) -> Element<'_, Message> {
        tab_buttons! {
            self;
            button0[0],
            button1[1],
            button2[2],
            button3[3],
            button4[4],
            button5[5],
            button6[6],
            button7[7],
            button8[8],
        }
        let tab_buttons = row![
            button0, button1, button2, button3, button4, button5, button6, button7, button8,
        ]
        .spacing(Self::TAB_SPACING);
        // let tab_buttons = self.tabs.iter().enumerate().fold(
        //     Row::new().spacing(Self::TAB_SPACING),
        //     |row, (index, tab)| {
        //         row.push(
        //             mouse_area(
        //                 button(rich_text![span(tab)].on_link_click(std::convert::identity))
        //                     .width(Self::TAB_WIDTH),
        //             )
        //             .on_press(Message::TabSelected(index)),
        //         )
        //     },
        // );

        let layout_text = lazy(&self.layout_symbol, |_| {
            let layout_text = container(
                rich_text![span(self.layout_symbol.clone())].on_link_click(std::convert::identity),
            )
            .center_x(Length::Shrink);
            layout_text
        });

        let scrollable_content = lazy(&self.layout_symbol, |_| {
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
            scrollable_content
        });

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
        let cpu_average = if let Some(snapshot) = self.system_monitor.get_snapshot() {
            snapshot.cpu_average
        } else {
            0.0
        };
        // info!("cpu_average: {cpu_average}");
        let cpu_usage_bar = progress_bar(0.0..=100.0, 100.0 - cpu_average)
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
            .girth(Self::TAB_HEIGHT * 0.5)
            .length(100.);

        let monitor_num = if let Some(monitor_info) = self.monitor_info_opt.as_ref() {
            monitor_info.monitor_num
        } else {
            0
        };
        let work_space_row = Row::new()
            .push(tab_buttons)
            .push(Space::with_width(3))
            .push(layout_text)
            .push(Space::with_width(3))
            .push(scrollable_content)
            .push(Space::with_width(Length::Fill))
            .push(cpu_usage_bar)
            .push(Space::with_width(3))
            .push(
                mouse_area(screenshot_text)
                    .on_enter(Message::MouseEnterScreenShot)
                    .on_exit(Message::MouseExitScreenShot)
                    .on_press(Message::LeftClick),
            )
            .push(Space::with_width(3))
            .push(time_button)
            .push(
                rich_text([
                    span(" "),
                    span(Self::monitor_num_to_icon(monitor_num as u8)),
                ])
                .on_link_click(std::convert::identity),
            )
            .align_y(iced::Alignment::Center);

        work_space_row.into()
    }

    fn view_under_line(&self) -> Element<'_, Message> {
        // 创建下划线行
        let mut underline_row = Row::new().spacing(Self::TAB_SPACING);
        for (index, _) in self.tabs.iter().enumerate() {
            // 创建下划线
            let tab_color = self.tab_colors.get(index).unwrap_or(&Self::DEFAULT_COLOR);

            // 根据状态设置样式
            if let Some(Some(tag_status)) = self
                .monitor_info_opt
                .as_ref()
                .map(|s| s.tag_status_vec.get(index))
            {
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
            .girth(3.0)
            .length(200.),
        );
        underline_row.into()
    }

    fn view(&self) -> Element<'_, Message> {
        // info!("view");
        // let work_space_row = self.view_work_space().explain(Color::from_rgb(1., 0., 1.));
        let work_space_row = self.view_work_space();

        let under_line_row = self.view_under_line();

        Column::new()
            .padding(2)
            .spacing(Self::TAB_SPACING)
            .push(work_space_row)
            .push(under_line_row)
            .into()
    }
}
