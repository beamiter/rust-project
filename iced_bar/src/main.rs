use chrono::Local;
use flexi_logger::{Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming};
use iced::time::{self, milliseconds};
use iced::widget::container;
use iced::widget::lazy;
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{Scrollable, Space, button, progress_bar, rich_text, row};
use iced::widget::{mouse_area, span};
use iced::{gradient, theme};

use iced::window::Id;
use iced::{
    Background, Border, Color, Degrees, Element, Length, Point, Radians, Size, Subscription, Task,
    Theme, border, color,
    widget::{Column, Row, text},
    window,
};
mod error;
pub use error::AppError;
use log::{error, info, warn};
use shared_structures::{
    CommandType, MonitorInfo, SharedCommand, SharedMessage, SharedRingBuffer, TagStatus,
};
use std::env;
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Once};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::audio_manager::AudioManager;
use crate::system_monitor::SystemMonitor;

pub mod audio_manager;
pub mod system_monitor;

static START: Once = Once::new();

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

fn adaptive_polling_worker(
    shared_buffer_opt_clone: Arc<Mutex<Option<SharedRingBuffer>>>,
    last_shared_message_opt_clone: Arc<Mutex<Option<SharedMessage>>>,
    message_received_clone: Arc<AtomicBool>,
    heartbeat_timestamp_clone: Arc<AtomicI64>,
    raw_window_id_clone: Arc<AtomicU64>,
) {
    info!("Starting adaptive polling worker thread");

    let mut prev_timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();

    let mut frame_count: u128 = 0;
    let mut consecutive_errors = 0;
    let mut consecutive_empty_reads = 0;
    let mut last_message_time = std::time::Instant::now();

    // 自适应睡眠参数
    let mut sleep_duration = Duration::from_millis(50); // 起始50ms
    const MIN_SLEEP: Duration = Duration::from_millis(10);
    const MAX_SLEEP: Duration = Duration::from_millis(500);
    const ACTIVE_THRESHOLD: Duration = Duration::from_secs(2); // 2秒内有消息认为是活跃状态

    loop {
        frame_count = frame_count.wrapping_add(1);
        let mut had_message = false;
        let tmp_now = std::time::Instant::now();
        let heartbeat_gap =
            Local::now().timestamp() - heartbeat_timestamp_clone.load(Ordering::Relaxed);
        // info!("heartbeat_gap: {heartbeat_gap}");
        if heartbeat_gap > 60 {
            // Should call resize
            //xdotool windowsize 0xc00004 800 40
            if let Err(_) = Command::new("xdotool")
                .arg("windowsize")
                .arg(format!(
                    "0x{:X}",
                    raw_window_id_clone.load(Ordering::Relaxed)
                ))
                .arg(800.to_string())
                .arg(40.to_string())
                .spawn()
            {
                error!("Failed to resize with xdotool");
            }
        }

        // 处理共享内存消息
        if let Ok(shared_buffer_lock) = shared_buffer_opt_clone.lock() {
            if let Some(shared_buffer) = &*shared_buffer_lock {
                match shared_buffer.try_read_latest_message::<SharedMessage>() {
                    Ok(Some(message)) => {
                        had_message = true;
                        consecutive_errors = 0;
                        consecutive_empty_reads = 0;
                        last_message_time = tmp_now;

                        if prev_timestamp != message.timestamp {
                            prev_timestamp = message.timestamp;

                            if let Ok(mut last_shared_message_lock) =
                                last_shared_message_opt_clone.lock()
                            {
                                match last_shared_message_lock.as_mut() {
                                    Some(last_shared_message) => {
                                        *last_shared_message = message;
                                    }
                                    None => {
                                        *last_shared_message_lock = Some(message);
                                    }
                                }
                                message_received_clone.store(true, Ordering::Relaxed);
                            }
                        }
                    }
                    Ok(None) => {
                        consecutive_errors = 0;
                        consecutive_empty_reads += 1;
                    }
                    Err(e) => {
                        consecutive_errors += 1;
                        if consecutive_errors == 1 || consecutive_errors % 100 == 0 {
                            log::warn!(
                                "Ring buffer read error: {} (count: {})",
                                e,
                                consecutive_errors
                            );
                        }
                        if consecutive_errors > 20 {
                            shared_buffer.reset_read_index();
                            consecutive_errors = 0;
                        }
                    }
                }
            }
        }

        // 自适应睡眠策略
        if had_message {
            // 有消息，使用最短睡眠
            sleep_duration = MIN_SLEEP;
        } else {
            let time_since_last_message = tmp_now.duration_since(last_message_time);
            if time_since_last_message < ACTIVE_THRESHOLD {
                // 最近有消息，使用较短的轮询间隔
                sleep_duration = Duration::from_millis(25);
            } else {
                // 长时间无消息，增加睡眠时间
                if consecutive_empty_reads > 20 {
                    sleep_duration = std::cmp::min(
                        Duration::from_millis(sleep_duration.as_millis() as u64 + 25),
                        MAX_SLEEP,
                    );
                }
            }
        }

        // 减少日志频率
        if frame_count % 5000 == 0 {
            log::debug!(
                "Frame {}, sleep: {:?}ms, empty_reads: {}",
                frame_count,
                sleep_duration.as_millis(),
                consecutive_empty_reads
            );
        }

        thread::sleep(sleep_duration);
    }
}

#[allow(dead_code)]
fn shared_memory_worker(
    shared_buffer_opt_clone: Arc<Mutex<Option<SharedRingBuffer>>>,
    last_shared_message_opt_clone: Arc<Mutex<Option<SharedMessage>>>,
    message_received_clone: Arc<AtomicBool>,
) {
    info!("Starting shared memory worker thread");
    let mut prev_timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();

    let mut frame_count: u128 = 0;
    let mut consecutive_errors = 0;
    loop {
        // 处理共享内存消息
        if let Ok(shared_buffer_lock) = shared_buffer_opt_clone.lock() {
            if let Some(shared_buffer) = &*shared_buffer_lock {
                match shared_buffer.try_read_latest_message::<SharedMessage>() {
                    Ok(Some(message)) => {
                        consecutive_errors = 0; // 成功读取，重置错误计数
                        if prev_timestamp != message.timestamp {
                            prev_timestamp = message.timestamp;
                            if let Ok(mut last_shared_message_lock) =
                                last_shared_message_opt_clone.lock()
                            {
                                if let Some(last_shared_message) = last_shared_message_lock.as_mut()
                                {
                                    *last_shared_message = message;
                                    if !message_received_clone.load(Ordering::SeqCst) {
                                        message_received_clone.store(true, Ordering::SeqCst);
                                    }
                                } else {
                                    *last_shared_message_lock = Some(message);
                                    if !message_received_clone.load(Ordering::SeqCst) {
                                        message_received_clone.store(true, Ordering::SeqCst);
                                    }
                                }
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
        }

        frame_count = frame_count.wrapping_add(1);
        thread::sleep(Duration::from_millis(10));
    }
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
        .font(include_bytes!("../fonts/NotoColorEmoji.ttf").as_slice())
        .window_size(Size::from([800., 40.]))
        .subscription(IcedBar::subscription)
        .title("iced_bar")
        // .style(IcedBar::style)
        .theme(|_| Theme::Light)
        .run()
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
enum Message {
    TabSelected(usize),
    LayoutClicked(u32),
    CheckSharedMessages,
    GetWindowId,
    GetWindowSize(Size),
    GetScaleFactor(f32),
    WindowIdReceived(Option<Id>),
    RawIdReceived(u64),
    ResizeWindow,
    ResizeWithId(Option<Id>),
    ShowSecondsToggle,

    // For mouse_area
    MouseEnterScreenShot,
    MouseExitScreenShot,
    LeftClick,
    RightClick,
}

struct IcedBar {
    active_tab: usize,
    tabs: [String; 9],
    tab_colors: [Color; 9],
    last_shared_message_opt: Arc<Mutex<Option<SharedMessage>>>,
    shared_buffer_opt: Arc<Mutex<Option<SharedRingBuffer>>>,
    message_count: u32,
    monitor_info_opt: Option<MonitorInfo>,
    now: chrono::DateTime<chrono::Local>,
    formated_now: String,
    heartbeat_timestamp: Arc<AtomicI64>,
    raw_window_id: Arc<AtomicU64>,
    current_window_id: Option<Id>,
    is_resized: bool,
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

    message_received: Arc<AtomicBool>,
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

        let local_shared_buffer_opt: Option<SharedRingBuffer> = if shared_path.is_empty() {
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
        let shared_buffer_opt = Arc::new(Mutex::new(local_shared_buffer_opt));
        let shared_buffer_opt_clone = shared_buffer_opt.clone();
        let last_shared_message_opt = Arc::new(Mutex::new(None::<SharedMessage>));
        let last_shared_message_opt_clone = last_shared_message_opt.clone();
        let message_received = Arc::new(AtomicBool::new(false));
        let message_received_clone = message_received.clone();
        let heartbeat_timestamp = Arc::new(AtomicI64::new(Local::now().timestamp()));
        let heartbeat_timestamp_clone = heartbeat_timestamp.clone();
        let raw_window_id = Arc::new(AtomicU64::new(0));
        let raw_window_id_clone = raw_window_id.clone();
        thread::spawn(move || {
            adaptive_polling_worker(
                shared_buffer_opt_clone,
                last_shared_message_opt_clone,
                message_received_clone,
                heartbeat_timestamp_clone,
                raw_window_id_clone,
            )
        });

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
            last_shared_message_opt,
            shared_buffer_opt,
            message_count: 0,
            monitor_info_opt: None,
            now: Local::now(),
            formated_now: String::new(),
            current_window_id: None,
            is_resized: false,
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
            message_received,
            heartbeat_timestamp,
            raw_window_id,
        }
    }

    fn send_tag_command(&mut self, is_view: bool) {
        let tag_bit = 1 << self.active_tab;
        let command = if is_view {
            SharedCommand::view_tag(tag_bit, self.monitor_num)
        } else {
            SharedCommand::toggle_tag(tag_bit, self.monitor_num)
        };

        if let Ok(shared_buffer_lock) = self.shared_buffer_opt.lock() {
            if let Some(shared_buffer) = &*shared_buffer_lock {
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

    fn send_layout_command(&mut self, layout_index: u32) {
        let command = SharedCommand::new(CommandType::SetLayout, layout_index, self.monitor_num);
        if let Ok(shared_buffer_lock) = self.shared_buffer_opt.lock() {
            if let Some(shared_buffer) = &*shared_buffer_lock {
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

            Message::MouseEnterScreenShot => {
                self.is_hovered = true;
                Task::none()
            }

            Message::ShowSecondsToggle => {
                self.show_seconds = !self.show_seconds;
                Task::none()
            }

            Message::MouseExitScreenShot => {
                self.is_hovered = false;
                self.mouse_position = None;
                Task::none()
            }

            Message::LeftClick => {
                let _ = Command::new("flameshot").arg("gui").spawn();
                Task::none()
            }

            Message::RightClick => Task::none(),

            Message::GetWindowSize(window_size) => {
                self.current_window_size = Some(window_size);
                if self.current_window_size.is_some() && self.target_window_size.is_some() {
                    let current_size = self.current_window_size.unwrap();
                    let target_size = self.target_window_size.unwrap();
                    if (current_size.width - target_size.width).abs() > 10.
                        || (current_size.height - target_size.height).abs() > 5.
                    {
                        info!(
                            "current_window_size: {:?}, target_window_size: {:?}",
                            self.current_window_size, self.target_window_size
                        );
                        self.is_resized = false;
                    }
                }
                let current_window_id = self.current_window_id;
                if !self.is_resized {
                    return Task::perform(async move { current_window_id }, Message::ResizeWithId);
                }
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
                    window::get_raw_id::<Message>(window_id.unwrap()).map(Message::RawIdReceived),
                ])
            }

            Message::RawIdReceived(raw_id) => {
                // Use xwininfo to get window id.
                // xdotool windowsize 0xc00004 800 40 work!
                self.raw_window_id.store(raw_id, Ordering::Relaxed);
                info!("{}", format!("RawIdReceived: 0x{:X}", raw_id));
                Task::none()
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
                self.current_window_id = window_id;
                if let Some(id) = self.current_window_id {
                    info!("ResizeWithId: {:?}, {:?}", window_id, self.monitor_info_opt);
                    let mut tasks = Vec::new();
                    if let Some(ref monitor_info) = self.monitor_info_opt {
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
                        self.target_window_pos = Some(window_pos);
                        self.target_window_size = Some(window_size);
                        self.is_resized = true;
                    }
                    Task::batch(tasks)
                } else {
                    window::get_latest().map(|id| Message::ResizeWithId(id))
                }
            }

            Message::CheckSharedMessages => {
                // 时间更新逻辑
                let tmp_now = Local::now();
                let format_str = if self.show_seconds {
                    "%Y-%m-%d %H:%M:%S"
                } else {
                    "%Y-%m-%d %H:%M"
                };
                let mut tasks = Vec::new();
                if tmp_now.timestamp() != self.now.timestamp() {
                    self.now = tmp_now;
                    self.heartbeat_timestamp
                        .store(tmp_now.timestamp(), Ordering::Release);
                    self.formated_now = tmp_now.format(format_str).to_string();
                    if let Some(window_id) = self.current_window_id {
                        tasks.push(window::get_size(window_id).map(Message::GetWindowSize));
                    }
                    // info!("CheckSharedMessages");
                }
                // 系统监控更新
                self.system_monitor.update_if_needed();
                self.audio_manager.update_if_needed();

                START.call_once(|| {
                    if self.current_window_id.is_none() {
                        tasks.push(Task::perform(async {}, |_| Message::GetWindowId));
                    }
                });
                if self.current_window_id.is_none() {
                    return Task::batch(tasks);
                }

                if self.message_received.load(Ordering::SeqCst) {
                    if let Some(last_shared_message) =
                        &*self.last_shared_message_opt.lock().unwrap()
                    {
                        self.message_count += 1;
                        self.monitor_info_opt = Some(last_shared_message.monitor_info.clone());
                    }
                    self.message_received.store(false, Ordering::SeqCst);

                    if let Some(monitor_info) = self.monitor_info_opt.as_ref() {
                        self.layout_symbol = monitor_info.ltsymbol.clone();
                        for (index, tag_status) in monitor_info.tag_status_vec.iter().enumerate() {
                            if tag_status.is_selected {
                                self.active_tab = index;
                            }
                        }
                    }

                    let current_window_id = self.current_window_id;
                    if !self.is_resized {
                        tasks.push(Task::perform(
                            async move { current_window_id },
                            Message::ResizeWithId,
                        ));
                    }
                }

                Task::batch(tasks)
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        time::every(milliseconds(50)).map(|_| Message::CheckSharedMessages)
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

    fn monitor_num_to_icon(monitor_num: u8) -> &'static str {
        match monitor_num {
            0 => "🥇",
            1 => "🥈",
            2 => "🥉",
            _ => "?",
        }
    }

    fn view_work_space(&self) -> Element<Message> {
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
        let screenshot_text =
            container(text(format!(" s {:.2} ", self.scale_factor.to_string())).center())
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

    fn view_under_line(&self) -> Element<Message> {
        // 创建下划线行
        let mut tag_status_vec: Vec<TagStatus> = Vec::new();
        if let Some(ref monitor_info) = self.monitor_info_opt {
            tag_status_vec = monitor_info.tag_status_vec.clone();
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
            .girth(3.0)
            .length(200.),
        );
        underline_row.into()
    }

    fn view(&self) -> Element<Message> {
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
