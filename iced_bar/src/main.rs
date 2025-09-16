use chrono::Local;
use flexi_logger::{Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming};
use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, Stream, StreamExt};
use iced::time::{self, milliseconds};
use iced::widget::container;
use iced::widget::{Space, button, rich_text};
use iced::widget::{mouse_area, span};
use iced::{Font, stream, theme};

use iced::window::Id;
use iced::{
    Background, Border, Color, Element, Length, Size, Subscription, Task, Theme, border, color,
    widget::{Column, Row, text},
    window,
};

use log::{debug, error, info, warn};
use std::env;
use std::process::Command;
use std::sync::Arc;
use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub mod audio_manager;
use audio_manager::AudioManager;
pub mod error;
use error::AppError;
use shared_structures::{CommandType, MonitorInfo, SharedCommand, SharedMessage, SharedRingBuffer};
pub mod system_monitor;
use system_monitor::SystemMonitor;

static _START: Once = Once::new();

// const NERD_FONT: Font = Font::with_name("SauceCodePro NerdFont Regular");
const NERD_FONT: Font = Font::with_name("NotoEmoji Regular");

/// Initialize logging system
fn initialize_logging(shared_path: &str) -> Result<(), AppError> {
    let tmp_now = Local::now();
    let timestamp = tmp_now.format("%Y-%m-%d_%H_%M_%S").to_string();

    let log_dir_candidates = [Some("/var/tmp/jwm".to_string())];

    let log_dir = log_dir_candidates
        .into_iter()
        .flatten()
        .find(|p| {
            std::fs::create_dir_all(p).ok();
            std::fs::metadata(p).map(|m| m.is_dir()).unwrap_or(false)
        })
        .unwrap_or_else(|| ".".to_string());

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
    let log_spec = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());

    Logger::try_with_str(log_spec)
        .map_err(|e| AppError::config(format!("Failed to create logger: {}", e)))?
        .format_for_files(flexi_logger::detailed_format)
        .format_for_stderr(flexi_logger::colored_opt_format)
        .log_to_file(
            FileSpec::default()
                .directory(&log_dir)
                .basename(log_filename)
                .suffix("log"),
        )
        .duplicate_to_stdout(Duplicate::Info)
        .rotate(
            Criterion::Size(10_000_000), // 10MB
            Naming::Numbers,
            Cleanup::KeepLogFiles(5),
        )
        .start()
        .map_err(|e| AppError::config(format!("Failed to start logger: {}", e)))?;

    info!("Log directory: {}", log_dir);
    Ok(())
}

fn main() -> iced::Result {
    let args: Vec<String> = env::args().collect();
    let application_id = "dev.iced.bar".to_string();
    let shared_path = args.get(1).cloned().unwrap_or_default();

    if let Err(e) = initialize_logging(&shared_path) {
        eprintln!("Failed to initialize logging: {}", e);
        std::process::exit(1);
    }

    iced::application(IcedBar::new, IcedBar::update, IcedBar::view)
        .window(window::Settings {
            platform_specific: window::settings::PlatformSpecific {
                application_id,
                ..Default::default()
            },
            size: Size::from([800., 40.]),
            decorations: false,
            transparent: true,
            level: window::Level::AlwaysOnTop,
            ..Default::default()
        })
        .default_font(NERD_FONT)
        .subscription(IcedBar::subscription)
        .title("iced_bar")
        .scale_factor(IcedBar::scale_factor)
        // .theme(|_| Theme::Light)
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
    ToggleLayoutSelector,
    ShowSecondsToggle,

    GetWindowId,
    WindowIdReceived(Option<Id>),

    GetScaleFactor(f32),

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
    current_window_id: Option<Id>,
    scale_factor: f32,
    is_hovered: bool,
    mouse_position: Option<iced::Point>,
    show_seconds: bool,
    layout_symbol: String,
    monitor_num: i32,

    // Audio + System
    audio_manager: AudioManager,
    system_monitor: SystemMonitor,

    transparent: bool,

    // throttle
    last_clock_update: Instant,
    last_monitor_update: Instant,

    // layout selector
    layout_selector_open: bool,
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
    const TAB_SPACING: f32 = 6.0;

    fn new() -> Self {
        let args: Vec<String> = env::args().collect();
        let shared_path = args.get(1).cloned().unwrap_or_default();

        let shared_buffer_opt =
            SharedRingBuffer::create_shared_ring_buffer_aux(&shared_path);

        Self {
            active_tab: 0,
            tabs: [
                "🏠".to_string(),
                "💻".to_string(),
                "🌐".to_string(),
                "🎵".to_string(),
                "📁".to_string(),
                "🎮".to_string(),
                "📧".to_string(),
                "🔧".to_string(),
                "📊".to_string(),
            ],
            tab_colors: [
                color!(0xFF6B6B), // red
                color!(0x4ECDC4), // cyan
                color!(0x45B7D1), // blue
                color!(0x96CEB4), // green
                color!(0xFECA57), // yellow
                color!(0xFF9FF3), // pink
                color!(0x54A0FF), // light blue
                color!(0x5F27CD), // purple
                color!(0x00D2D3), // teal
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
            layout_symbol: "[]=".to_string(),
            monitor_num: 0,
            audio_manager: AudioManager::new(),
            system_monitor: SystemMonitor::new(5),
            transparent: true,
            last_clock_update: Instant::now(),
            last_monitor_update: Instant::now(),
            layout_selector_open: false,
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
            stream::channel(100, move |mut output: mpsc::Sender<Message>| async move {
                if path.is_empty() {
                    let _ = output
                        .send(Message::SharedMemoryError("Empty shared path".to_string()))
                        .await;
                    return;
                }

                let shared_buffer = match SharedRingBuffer::open_aux(&path, None) {
                    Ok(buffer) => Arc::new(buffer),
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

                let (mut tx, mut rx) = mpsc::channel::<Message>(100);
                let buffer_clone = shared_buffer.clone();
                let stop = Arc::new(AtomicBool::new(false));
                let stop_c = stop.clone();

                std::thread::spawn(move || {
                    let mut prev_timestamp: u128 = 0;
                    while !stop_c.load(Ordering::Relaxed) {
                        match buffer_clone.wait_for_message(Some(Duration::from_secs(2))) {
                            Ok(true) => {
                                if let Ok(Some(message)) = buffer_clone.try_read_latest_message() {
                                    let ts: u128 = message.timestamp as u128;
                                    if prev_timestamp != ts {
                                        prev_timestamp = ts;
                                        if tx
                                            .try_send(Message::SharedMemoryUpdated(message))
                                            .is_err()
                                        {
                                            break;
                                        }
                                    }
                                }
                            }
                            Ok(false) => { /* timeout */ }
                            Err(e) => {
                                let _ = tx.try_send(Message::SharedMemoryError(format!(
                                    "Wait for message failed: {}",
                                    e
                                )));
                                break;
                            }
                        }
                    }
                });

                while let Some(msg) = rx.next().await {
                    let _ = output.send(msg).await;
                }

                stop.store(true, Ordering::Relaxed);
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
                Ok(true) => info!("Sent command: {:?} by shared_buffer", command),
                Ok(false) => warn!("Command buffer full, command dropped"),
                Err(e) => error!("Failed to send command: {}", e),
            }
        }
    }

    fn send_layout_command(&mut self, layout_index: u32) {
        let command = SharedCommand::new(CommandType::SetLayout, layout_index, self.monitor_num);
        if let Some(shared_buffer) = &self.shared_buffer_opt {
            match shared_buffer.send_command(command) {
                Ok(true) => info!("Sent command: {:?} by shared_buffer", command),
                Ok(false) => warn!("Command buffer full, command dropped"),
                Err(e) => error!("Failed to send command: {}", e),
            }
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
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
                self.layout_selector_open = false;
                Task::none()
            }

            Message::ToggleLayoutSelector => {
                self.layout_selector_open = !self.layout_selector_open;
                Task::none()
            }

            Message::GetWindowId => {
                info!("GetWindowId");
                window::latest().map(Message::WindowIdReceived)
            }

            Message::WindowIdReceived(window_id) => {
                if let Some(wid) = window_id {
                    info!("WindowIdReceived: {:?}", wid);
                    self.current_window_id = Some(wid);
                    Task::batch([window::scale_factor(wid).map(Message::GetScaleFactor)])
                } else {
                    warn!("WindowId not available yet");
                    Task::none()
                }
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
                if let Err(e) = Command::new("flameshot").arg("gui").spawn() {
                    warn!("Failed to spawn flameshot: {e}");
                }
                Task::none()
            }

            Message::RightClick => Task::none(),

            Message::GetScaleFactor(scale_factor) => {
                info!("scale_factor: {}", scale_factor);
                self.scale_factor = scale_factor;
                Task::none()
            }

            Message::UpdateTime => {
                if self.last_clock_update.elapsed() >= Duration::from_millis(900) {
                    let tmp_now = Local::now();
                    let format_str = if self.show_seconds {
                        "%Y-%m-%d %H:%M:%S"
                    } else {
                        "%Y-%m-%d %H:%M"
                    };
                    self.formated_now = tmp_now.format(format_str).to_string();
                    self.last_clock_update = Instant::now();
                }

                if self.last_monitor_update.elapsed() >= Duration::from_secs(2) {
                    self.system_monitor.update_if_needed();
                    self.audio_manager.update_if_needed();
                    self.last_monitor_update = Instant::now();
                }

                Task::none()
            }

            Message::SharedMemoryUpdated(message) => {
                debug!("SharedMemoryUpdated: {:?}", message.timestamp);
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
                Task::none()
            }

            Message::SharedMemoryError(err) => {
                warn!("SharedMemoryError: {err}");
                Task::none()
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        if self.current_window_id.is_none() {
            Subscription::run(Self::prepare_worker)
        } else {
            let clock = time::every(milliseconds(1000)).map(|_| Message::UpdateTime);
            let shared = if self.shared_path.is_empty() {
                Subscription::none()
            } else {
                Self::message_notify_subscription(self.shared_path.clone())
            };
            Subscription::batch(vec![clock, shared])
        }
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

    fn scale_factor(&self) -> f32 {
        1.0 / self.scale_factor
    }

    fn monitor_num_to_icon(monitor_num: u8) -> &'static str {
        match monitor_num {
            0 => "󰎡",
            1 => "󰎤",
            _ => "?",
        }
    }

    // -------- Workspace pills --------
    fn tag_visuals(&self, index: usize) -> (Color, f32, Color) {
        // returns (background, border_width, border_color)
        let tag_color = self
            .tab_colors
            .get(index)
            .copied()
            .unwrap_or(Self::DEFAULT_COLOR);

        if let Some(monitor) = self.monitor_info_opt.as_ref() {
            if let Some(status) = monitor.tag_status_vec.get(index) {
                if status.is_urg {
                    // urgent: red bg + bold violet border
                    return (
                        Color::from_rgba(1.0, 0.0, 0.0, 0.80),
                        2.5,
                        Color::from_rgba(0.54, 0.17, 0.89, 1.0), // violet
                    );
                } else if status.is_filled {
                    // filled: solid tag color + bold border
                    return (tag_color.scale_alpha(1.0), 2.0, tag_color);
                } else if status.is_selected {
                    // selected: semi tag color + thin border
                    return (tag_color.scale_alpha(0.7), 1.5, tag_color);
                } else if status.is_occ {
                    // occupied: faint bg, no border
                    return (tag_color.scale_alpha(0.4), 0.0, Color::TRANSPARENT);
                }
            }
        }

        // default: transparent bg, no border
        (Color::TRANSPARENT, 0.0, Color::TRANSPARENT)
    }

    fn workspace_button<'a>(
        &self,
        index: usize,
        label: &'a str,
    ) -> iced::widget::Button<'a, Message> {
        let (bg, border_w, border_c) = self.tag_visuals(index);

        let radius = 6.0;
        button(
            rich_text![span(label.to_string())]
                .size(18)
                .on_link_click(std::convert::identity),
        )
        .padding([4, 8])
        .width(Self::TAB_WIDTH)
        .height(Self::TAB_HEIGHT + 4.)
        .style(move |_theme: &Theme, status: button::Status| {
            let mut background = bg;
            let mut border_width = border_w;

            match status {
                button::Status::Hovered => {
                    // stronger border on hover
                    border_width = (border_w + 1.0).min(3.0);
                    if background.a > 0.0 {
                        background.a = (background.a + 0.08).min(1.0);
                    } else {
                        // subtle hover when transparent
                        background = Color::from_rgba(1.0, 1.0, 1.0, 0.08);
                    }
                }
                button::Status::Pressed => {
                    // pressed -> slightly darker
                    if background.a > 0.0 {
                        background.a = (background.a + 0.12).min(1.0);
                    } else {
                        background = Color::from_rgba(0.9, 0.9, 0.9, 0.10);
                    }
                }
                _ => {}
            }

            button::Style {
                background: Some(Background::Color(background)),
                text_color: Color::BLACK,
                border: Border {
                    color: border_c,
                    width: border_width,
                    radius: border::Radius::from(radius),
                },
                ..Default::default()
            }
        })
        .on_press(Message::TabSelected(index))
    }

    fn layout_toggle_button<'a>(&self) -> iced::widget::Button<'a, Message> {
        let is_open = self.layout_selector_open;
        let color_open = Color::from_rgb(0.24, 0.70, 0.44); // success
        let color_close = Color::from_rgb(0.85, 0.33, 0.0); // error

        let pill_color = if is_open { color_open } else { color_close };
        let label = self.layout_symbol.clone();

        button(rich_text![span(label)].on_link_click(std::convert::identity))
            .padding([1, 8])
            .style(move |_theme: &Theme, status: button::Status| {
                let mut bg = pill_color.scale_alpha(0.85);
                let mut border_w = 1.0;

                if matches!(status, button::Status::Hovered) {
                    bg.a = 1.0;
                    border_w = 2.0;
                }

                button::Style {
                    background: Some(Background::Color(bg)),
                    text_color: Color::WHITE,
                    border: Border {
                        color: pill_color,
                        width: border_w,
                        radius: border::Radius::from(6.0),
                    },
                    ..Default::default()
                }
            })
            .on_press(Message::ToggleLayoutSelector)
    }

    fn layout_options_row(&self) -> Element<'_, Message> {
        // Available layouts: 0 = "[]=", 1 = "><>", 2 = "[M]"
        let layouts: [(&str, u32); 3] = [("[]=", 0), ("><>", 1), ("[M]", 2)];
        let current = self.layout_symbol.as_str();

        let mut row = Row::new().spacing(Self::TAB_SPACING);
        for (sym, idx) in layouts {
            let is_current = sym == current;

            let btn = button(text(sym))
                .padding([1, 6])
                .style(move |_theme: &Theme, status: button::Status| {
                    let base = if is_current {
                        Color::from_rgb(0.24, 0.70, 0.44) // success
                    } else {
                        Color::from_rgb(0.25, 0.41, 0.88) // royal blue
                    };

                    let mut bg = base.scale_alpha(0.85);
                    let mut border_w = if is_current { 2.0 } else { 1.0 };

                    if matches!(status, button::Status::Hovered) {
                        bg.a = 1.0;
                        border_w += 1.0;
                    }

                    button::Style {
                        background: Some(Background::Color(bg)),
                        text_color: Color::WHITE,
                        border: Border {
                            color: base,
                            width: border_w,
                            radius: border::Radius::from(6.0),
                        },
                        ..Default::default()
                    }
                })
                .on_press(Message::LayoutClicked(idx));

            row = row.push(btn);
        }

        row.into()
    }

    fn view_work_space(&self) -> Element<'_, Message> {
        // Workspace pills
        let mut tags_row = Row::new().spacing(Self::TAB_SPACING * 0.5);
        for (index, label) in self.tabs.iter().enumerate() {
            tags_row = tags_row
                .push(self.workspace_button(index, label))
                .align_y(iced::Alignment::Center);
        }

        // Layout section: main button + optional selector row
        let layout_button = self.layout_toggle_button();

        let layout_selector = if self.layout_selector_open {
            let selector = self.layout_options_row();
            Row::new()
                .spacing(Self::TAB_SPACING)
                .push(selector)
                .align_y(iced::Alignment::Center)
        } else {
            Row::new().into()
        };

        // Screenshot pill with hover effect
        let is_hovered = self.is_hovered;
        let screenshot_pill = container(
            text(format!("📸 {:.2}", self.scale_factor))
                .size(16)
                .center(),
        )
        .height(Self::TAB_HEIGHT)
        .padding([4, 8])
        .style(move |_theme: &Theme| {
            if is_hovered {
                container::Style {
                    background: Some(Background::Color(Color::from_rgb(1.0, 0.5, 0.0))), // 橙色背景
                    text_color: Some(Color::WHITE),
                    border: Border {
                        radius: border::radius(12.0),
                        width: 1.0,
                        color: Color::from_rgb(1.0, 0.5, 0.0),
                    },
                    ..Default::default()
                }
            } else {
                container::Style {
                    background: Some(Background::Color(Color::from_rgb(0.0, 0.8, 0.8))), // 青色背景
                    text_color: Some(Color::WHITE),
                    border: Border {
                        radius: border::radius(12.0),
                        width: 1.0,
                        color: Color::from_rgb(0.0, 0.8, 0.8),
                    },
                    ..Default::default()
                }
            }
        });

        // CPU usage pill
        let cpu_usage = if let Some(snapshot) = self.system_monitor.get_snapshot() {
            snapshot.cpu_average
        } else {
            0.0
        };

        let cpu_pill = self.create_usage_pill("CPU", cpu_usage);

        // Memory usage pill
        let (memory_total_gb, memory_used_gb) =
            if let Some(snapshot) = self.system_monitor.get_snapshot() {
                (
                    snapshot.memory_total as f32 / 1e9,
                    snapshot.memory_used as f32 / 1e9,
                )
            } else {
                (0.0, 0.0)
            };

        let memory_usage = if memory_total_gb > 0.0 {
            (memory_used_gb / memory_total_gb) * 100.0
        } else {
            0.0
        };

        let memory_pill = self.create_usage_pill("MEM", memory_usage);

        // Time pill with enhanced styling
        let time_pill = container(text(format!("🕐 {}", self.formated_now)).size(18).center())
            .padding([1, 8])
            .style(|_theme: &Theme| {
                container::Style {
                    background: Some(Background::Color(Color::from_rgb(0.3, 0.6, 0.9))), // 蓝色背景
                    text_color: Some(Color::WHITE),
                    border: Border {
                        radius: border::radius(12.0),
                        width: 1.0,
                        color: Color::from_rgb(0.3, 0.6, 0.9),
                    },
                    ..Default::default()
                }
            });

        let monitor_num = if let Some(monitor_info) = self.monitor_info_opt.as_ref() {
            monitor_info.monitor_num
        } else {
            0
        };

        // Monitor indicator pill
        let monitor_pill = container(
            text(format!(
                "🖥️ {}",
                Self::monitor_num_to_icon(monitor_num as u8)
            ))
            .size(18)
            .center(),
        )
        .padding([1, 8])
        .style(|_theme: &Theme| {
            container::Style {
                background: Some(Background::Color(Color::from_rgb(0.6, 0.4, 0.8))), // 紫色背景
                text_color: Some(Color::WHITE),
                border: Border {
                    radius: border::radius(12.0),
                    width: 1.0,
                    color: Color::from_rgb(0.6, 0.4, 0.8),
                },
                ..Default::default()
            }
        });

        Row::new()
            .push(tags_row)
            .push(Space::with_width(6))
            .push(layout_button)
            .push(Space::with_width(6))
            .push(layout_selector)
            .push(Space::with_width(Length::Fill))
            .push(cpu_pill)
            .push(Space::with_width(6))
            .push(memory_pill)
            .push(Space::with_width(6))
            .push(
                mouse_area(screenshot_pill)
                    .on_enter(Message::MouseEnterScreenShot)
                    .on_exit(Message::MouseExitScreenShot)
                    .on_press(Message::LeftClick),
            )
            .push(Space::with_width(6))
            .push(mouse_area(time_pill).on_press(Message::ShowSecondsToggle))
            .push(Space::with_width(6))
            .push(monitor_pill)
            .push(Space::with_width(6))
            .align_y(iced::Alignment::Center)
            .into()
    }

    // 新增辅助方法：创建使用率pill
    fn create_usage_pill(&self, label: &str, usage_percent: f32) -> Element<'_, Message> {
        let usage = usage_percent.clamp(0.0, 100.0);

        // 根据使用率选择颜色
        let (bg_color, text_color) = self.get_usage_colors(usage);

        container(text(format!("{} {:.0}%", label, usage)).size(18).center())
            .padding([1, 8])
            .style(move |_theme: &Theme| {
                container::Style {
                    background: Some(Background::Color(bg_color)),
                    text_color: Some(text_color),
                    border: Border {
                        radius: border::radius(12.0), // 圆角pill样式
                        width: 1.0,
                        color: bg_color,
                    },
                    ..Default::default()
                }
            })
            .into()
    }

    // 新增辅助方法：根据使用率获取颜色
    fn get_usage_colors(&self, usage_percent: f32) -> (Color, Color) {
        match usage_percent {
            // 0-30%: 绿色 (良好)
            usage if usage <= 30.0 => (
                Color::from_rgb(0.2, 0.8, 0.2), // 绿色背景
                Color::WHITE,                   // 白色文字
            ),
            // 30-60%: 黄色 (注意)
            usage if usage <= 60.0 => (
                Color::from_rgb(1.0, 0.8, 0.0), // 黄色背景
                Color::BLACK,                   // 黑色文字
            ),
            // 60-80%: 橙色 (警告)
            usage if usage <= 80.0 => (
                Color::from_rgb(1.0, 0.6, 0.0), // 橙色背景
                Color::WHITE,                   // 白色文字
            ),
            // 80-100%: 红色 (危险)
            _ => (
                Color::from_rgb(0.9, 0.2, 0.2), // 红色背景
                Color::WHITE,                   // 白色文字
            ),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let work_space_row = self.view_work_space();

        Column::new()
            .padding(4)
            .spacing(Self::TAB_SPACING)
            .push(work_space_row)
            .into()
    }
}
