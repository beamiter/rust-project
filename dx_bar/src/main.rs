use chrono::Local;
use dioxus::{
    desktop::{Config, WindowBuilder},
    prelude::*,
};
use flexi_logger::{Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming};
use log::{error, info, warn};
use shared_structures::{SharedCommand, SharedMessage, SharedRingBuffer};
use std::{
    env,
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
// 导入 tao 用于窗口配置
use tao::dpi::{LogicalPosition, LogicalSize};

mod error;
pub use error::AppError;
mod system_monitor;
use system_monitor::{SystemMonitor, SystemSnapshot};

// 在编译时直接包含CSS文件
const STYLE_CSS: &str = include_str!("../assets/style.css");

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

// 优化的共享内存工作线程 - 降低CPU使用率
fn shared_memory_worker(
    shared_path: String,
    message_sender: mpsc::Sender<SharedMessage>,
    command_receiver: mpsc::Receiver<SharedCommand>,
) {
    info!("Starting shared memory worker thread");

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

    let mut consecutive_errors = 0;
    let mut last_message_read = Instant::now();

    const POLL_INTERVAL: Duration = Duration::from_millis(5);
    const MAX_IDLE_TIME: Duration = Duration::from_secs(1);

    loop {
        let loop_start = Instant::now();

        let mut has_commands = false;
        while let Ok(cmd) = command_receiver.try_recv() {
            has_commands = true;
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

        if let Some(ref shared_buffer) = shared_buffer_opt {
            match shared_buffer.try_read_latest_message::<SharedMessage>() {
                Ok(Some(message)) => {
                    consecutive_errors = 0;
                    last_message_read = Instant::now();

                    if prev_timestamp != message.timestamp {
                        prev_timestamp = message.timestamp;
                        if let Err(e) = message_sender.send(message) {
                            error!("Failed to send message: {}", e);
                            break;
                        }
                    }
                }
                Ok(None) => {
                    consecutive_errors = 0;
                }
                Err(e) => {
                    consecutive_errors += 1;
                    if consecutive_errors == 1 || consecutive_errors % 50 == 0 {
                        error!(
                            "Ring buffer read error ({}): {}. Buffer state: available={}, last_timestamp={}",
                            consecutive_errors,
                            e,
                            shared_buffer.available_messages(),
                            shared_buffer.get_last_timestamp()
                        );
                    }

                    if consecutive_errors > 50 {
                        warn!("Too many consecutive errors, resetting read index");
                        shared_buffer.reset_read_index();
                        consecutive_errors = 0;
                    }
                }
            }
        }

        let mut sleep_duration = POLL_INTERVAL;
        if !has_commands && last_message_read.elapsed() > MAX_IDLE_TIME {
            sleep_duration = Duration::from_millis(10);
        }

        let elapsed = loop_start.elapsed();
        if elapsed < sleep_duration {
            thread::sleep(sleep_duration - elapsed);
        }
    }

    info!("Shared memory worker thread exiting");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let instance_name = args
        .get(0)
        .cloned()
        .or_else(|| env::var("DX_BAR_INSTANCE").ok())
        .unwrap_or_else(|| "dx_bar_default".to_string());
    info!("instance_name: {instance_name}");
    let shared_path = args.get(1).cloned().unwrap_or_default();

    if let Err(e) = initialize_logging(&shared_path) {
        error!("Failed to initialize logging: {}", e);
        std::process::exit(1);
    }

    info!("Starting dx_bar v{}", 1.0);

    dioxus::LaunchBuilder::desktop()
        .with_cfg(
            Config::new().with_window(
                WindowBuilder::new()
                    .with_title("dx_bar")
                    .with_inner_size(LogicalSize::new(1980, 50))
                    .with_position(LogicalPosition::new(0, 0))
                    .with_maximizable(false)
                    .with_minimizable(false)
                    .with_visible_on_all_workspaces(true)
                    .with_decorations(false)
                    .with_always_on_top(true),
            ),
        )
        .launch(App);
}

// 将按钮数据定义为静态常量
const BUTTONS: &[&str] = &["🔴", "🟠", "🟡", "🟢", "🔵", "🟣", "🟤", "⚪", "⚫", "🌈"];

// 定义按钮状态枚举
#[derive(Debug, Clone, PartialEq)]
enum ButtonState {
    Filtered,
    Selected,
    Urgent,
    Occupied,
    Default,
}

impl ButtonState {
    fn from_flags(is_filtered: bool, is_selected: bool, is_urg: bool, is_occ: bool) -> Self {
        if is_filtered {
            ButtonState::Filtered
        } else if is_selected {
            ButtonState::Selected
        } else if is_urg {
            ButtonState::Urgent
        } else if is_occ {
            ButtonState::Occupied
        } else {
            ButtonState::Default
        }
    }

    fn to_css_class(&self) -> &'static str {
        match self {
            ButtonState::Filtered => "emoji-button state-filtered",
            ButtonState::Selected => "emoji-button state-selected",
            ButtonState::Urgent => "emoji-button state-urgent",
            ButtonState::Occupied => "emoji-button state-occupied",
            ButtonState::Default => "emoji-button state-default",
        }
    }
}

// 按钮状态数据结构
#[derive(Debug, Clone, Default, PartialEq)]
struct ButtonStateData {
    is_filtered: bool,
    is_selected: bool,
    is_urg: bool,
    is_occ: bool,
}

impl ButtonStateData {
    fn get_state(&self) -> ButtonState {
        ButtonState::from_flags(self.is_filtered, self.is_selected, self.is_urg, self.is_occ)
    }
}

fn get_button_class(index: usize, button_states: &[ButtonStateData]) -> &'static str {
    if index < button_states.len() {
        button_states[index].get_state().to_css_class()
    } else {
        "emoji-button state-default"
    }
}

/// 格式化字节为人类可读的格式
fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{:.0}{}", size, UNITS[unit_index])
    } else {
        format!("{:.1}{}", size, UNITS[unit_index])
    }
}

/// 系统信息显示组件
#[component]
fn SystemInfoDisplay(snapshot: Option<SystemSnapshot>) -> Element {
    if let Some(ref snap) = snapshot {
        let cpu_color = if snap.cpu_average > 80.0 {
            "#dc3545" // 红色
        } else if snap.cpu_average > 60.0 {
            "#ffc107" // 黄色
        } else {
            "#28a745" // 绿色
        };

        let mem_color = if snap.memory_usage_percent > 85.0 {
            "#dc3545" // 红色
        } else if snap.memory_usage_percent > 70.0 {
            "#ffc107" // 黄色
        } else {
            "#28a745" // 绿色
        };

        let memory_text = format_bytes(snap.memory_used);
        let memory_total_text = format_bytes(snap.memory_total);

        // 电池相关
        let battery_color = if snap.battery_percent > 50.0 {
            "#28a745" // 绿色
        } else if snap.battery_percent > 20.0 {
            "#ffc107" // 黄色
        } else {
            "#dc3545" // 红色
        };

        let battery_icon = if snap.is_charging {
            "🔌" // 充电中
        } else if snap.battery_percent > 75.0 {
            "🔋" // 满电
        } else if snap.battery_percent > 50.0 {
            "🔋" // 较满
        } else if snap.battery_percent > 25.0 {
            "🔋" // 一般
        } else {
            "🪫" // 低电量
        };

        rsx! {
            div {
                class: "system-info-container",
                
                // CPU 使用率
                div {
                    class: "system-metric",
                    title: "CPU 平均使用率",
                    
                    span { class: "metric-icon", "💻" }
                    span { 
                        class: "metric-value",
                        style: "color: {cpu_color};",
                        "{snap.cpu_average:.1}%"
                    }
                }

                // 内存使用情况  
                div {
                    class: "system-metric",
                    title: "内存使用: {memory_text} / {memory_total_text}",
                    
                    span { class: "metric-icon", "🧠" }
                    span { 
                        class: "metric-value",
                        style: "color: {mem_color};",
                        "{snap.memory_usage_percent:.1}%"
                    }
                }

                // 电池状态
                div {
                    class: "system-metric",
                    title: if snap.is_charging { 
                        format!("电池充电中: {:.1}%", snap.battery_percent)
                    } else { 
                        format!("电池电量: {:.1}%", snap.battery_percent)
                    },
                    
                    span { class: "metric-icon", "{battery_icon}" }
                    span { 
                        class: "metric-value",
                        style: "color: {battery_color};",
                        "{snap.battery_percent:.0}%"
                    }
                }
            }
        }
    } else {
        rsx! {
            div {
                class: "system-info-container",
                
                div {
                    class: "system-metric",
                    span { class: "metric-icon", "💻" }
                    span { class: "metric-value", "--%" }
                }

                div {
                    class: "system-metric",
                    span { class: "metric-icon", "🧠" }
                    span { class: "metric-value", "--%" }
                }

                div {
                    class: "system-metric",
                    span { class: "metric-icon", "🔋" }
                    span { class: "metric-value", "--%" }
                }
            }
        }
    }
}

/// 时间组件
#[component]
fn TimeDisplay(show_seconds: bool) -> Element {
    let mut current_time = use_signal(|| Local::now());

    // 时间更新循环
    use_effect(move || {
        spawn(async move {
            loop {
                // 根据是否显示秒来决定更新频率
                let update_interval = if show_seconds {
                    Duration::from_millis(1000) // 显示秒时每秒更新
                } else {
                    Duration::from_millis(60000) // 不显示秒时每分钟更新
                };

                tokio::time::sleep(update_interval).await;
                current_time.set(Local::now());
            }
        });
    });

    let time_format = if show_seconds { "%H:%M:%S" } else { "%H:%M" };
    let time_str = current_time().format(time_format).to_string();

    rsx! {
        div {
            class: "time-display",
            onclick: move |_| {
                info!("Time clicked - current format includes seconds: {}", show_seconds);
            },
            "{time_str}"
        }
    }
}

#[component]
fn App() -> Element {
    // 按钮状态数组
    let mut button_states = use_signal(|| vec![ButtonStateData::default(); BUTTONS.len()]);
    let mut last_update = use_signal(|| Instant::now());

    // 时间显示秒数的状态
    let mut show_seconds = use_signal(|| true); // 默认显示秒

    // 系统信息状态
    let mut system_snapshot = use_signal(|| None::<SystemSnapshot>);

    // 初始化系统监控
    use_effect(move || {
        spawn(async move {
            // 在独立的线程中运行系统监控，避免阻塞UI
            let (sys_sender, sys_receiver) = std::sync::mpsc::channel();
            
            thread::spawn(move || {
                let mut monitor = SystemMonitor::new(30); // 保存30个历史数据点
                monitor.set_update_interval(Duration::from_millis(2000)); // 2秒更新一次

                loop {
                    monitor.update_if_needed();
                    
                    if let Some(snapshot) = monitor.get_snapshot() {
                        if sys_sender.send(snapshot.clone()).is_err() {
                            // 接收端已关闭，退出线程
                            break;
                        }
                    }

                    thread::sleep(Duration::from_millis(500));
                }
            });

            // 在异步任务中接收系统信息更新
            loop {
                if let Ok(snapshot) = sys_receiver.try_recv() {
                    system_snapshot.set(Some(snapshot));
                }
                
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });
    });

    // 初始化共享内存通信
    use_effect(move || {
        let (message_sender, message_receiver) = mpsc::channel::<SharedMessage>();
        let (_command_sender, command_receiver) = mpsc::channel::<SharedCommand>();

        let shared_path = std::env::args().nth(1).unwrap_or_else(|| {
            std::env::var("SHARED_MEMORY_PATH").unwrap_or_else(|_| "/dev/shm/monitor_0".to_string())
        });

        info!("Using shared memory path: {}", shared_path);

        let shared_path_clone = shared_path.clone();
        thread::spawn(move || {
            shared_memory_worker(shared_path_clone, message_sender, command_receiver);
        });

        spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(200));

            loop {
                interval.tick().await;

                let mut latest_message = None;
                let mut message_count = 0;

                while let Ok(message) = message_receiver.try_recv() {
                    latest_message = Some(message);
                    message_count += 1;

                    if message_count >= 5 {
                        break;
                    }
                }

                if let Some(shared_message) = latest_message {
                    let now = Instant::now();

                    if now.duration_since(last_update()) >= Duration::from_millis(150) {
                        let mut new_states = vec![ButtonStateData::default(); BUTTONS.len()];

                        for (index, tag_status) in shared_message
                            .monitor_info
                            .tag_status_vec
                            .iter()
                            .enumerate()
                        {
                            if index < new_states.len() {
                                new_states[index] = ButtonStateData {
                                    is_filtered: tag_status.is_filled,
                                    is_selected: tag_status.is_selected,
                                    is_urg: tag_status.is_urg,
                                    is_occ: tag_status.is_occ,
                                };
                            }
                        }

                        let current_states = button_states.read().clone();
                        if *current_states != new_states {
                            button_states.set(new_states);
                            last_update.set(now);
                        }
                    }
                }
            }
        });
    });

    rsx! {
        document::Style { "{STYLE_CSS}" }

        div {
            class: "button-row",

            // 按钮区域 - 左侧
            div {
                class: "buttons-container",
                for (i, emoji) in BUTTONS.iter().enumerate() {
                    button {
                        key: "{i}",
                        class: get_button_class(i, &button_states()),
                        "{emoji}"
                    }
                }
            }

            // 右侧信息区域
            div {
                class: "right-info-container",
                
                // 系统信息显示
                SystemInfoDisplay { snapshot: system_snapshot() }

                // 时间显示
                div {
                    class: "time-container",
                    onclick: move |_| {
                        show_seconds.set(!show_seconds());
                        info!("Toggle seconds display: {}", show_seconds());
                    },
                    TimeDisplay { show_seconds: show_seconds() }
                }
            }
        }
    }
}
