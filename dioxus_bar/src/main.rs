use chrono::Local;
use dioxus::{
    desktop::{
        Config, LogicalPosition, LogicalSize, WindowBuilder,
        tao::{event_loop::EventLoopBuilder, platform::unix::EventLoopBuilderExtUnix},
        use_window,
    },
    prelude::*,
};
use flexi_logger::{Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming};
use log::{debug, error, info, warn};
use shared_structures::{SharedCommand, SharedMessage, SharedRingBuffer};
use std::{
    env,
    process::Command,
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

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
        "dioxus_bar".to_string()
    } else {
        std::path::Path::new(shared_path)
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| format!("dioxus_bar_{}", name))
            .unwrap_or_else(|| "dioxus_bar".to_string())
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

// 优化的共享内存工作线程 - 降低CPU使用率
fn shared_memory_worker(shared_path: String, message_sender: mpsc::Sender<SharedMessage>) {
    info!("Starting shared memory worker thread");
    let shared_buffer_opt = SharedRingBuffer::create_shared_ring_buffer(&shared_path);
    let mut prev_timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    if let Some(ref shared_buffer) = shared_buffer_opt {
        loop {
            match shared_buffer.wait_for_message(Some(std::time::Duration::from_secs(2))) {
                Ok(true) => {
                    if let Ok(Some(message)) = shared_buffer.try_read_latest_message() {
                        if prev_timestamp != message.timestamp.into() {
                            prev_timestamp = message.timestamp.into();
                            if let Err(e) = message_sender.send(message) {
                                error!("Failed to send message: {}", e);
                                break;
                            }
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
    }

    info!("Shared memory worker task exiting");
}

fn send_tag_command(
    shared_buffer_opt: &Option<SharedRingBuffer>,
    monitor_id: i32,
    active_tab: usize,
    is_view: bool,
) {
    let tag_bit = 1 << active_tab;
    let cmd = if is_view {
        SharedCommand::view_tag(tag_bit, monitor_id)
    } else {
        SharedCommand::toggle_tag(tag_bit, monitor_id)
    };
    if let Some(shared_buffer) = shared_buffer_opt {
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

fn main() {
    let args: Vec<String> = env::args().collect();
    let shared_path = args.get(1).cloned().unwrap_or_default();
    let mut instance_name = shared_path.replace("/dev/shm/monitor_", "dioxus_bar_");
    if instance_name.is_empty() {
        instance_name = "dioxus_bar".to_string();
    }
    if let Err(e) = initialize_logging(&shared_path) {
        error!("Failed to initialize logging: {}", e);
        std::process::exit(1);
    }
    // 检查环境信息
    info!("=== Environment Debug Info ===");
    info!("DISPLAY: {:?}", env::var("DISPLAY"));
    info!("WAYLAND_DISPLAY: {:?}", env::var("WAYLAND_DISPLAY"));
    info!("XDG_SESSION_TYPE: {:?}", env::var("XDG_SESSION_TYPE"));
    info!("DESKTOP_SESSION: {:?}", env::var("DESKTOP_SESSION"));
    info!("XDG_CURRENT_DESKTOP: {:?}", env::var("XDG_CURRENT_DESKTOP"));
    // 检查屏幕分辨率（如果可能）
    if let Ok(output) = Command::new("xrandr").arg("--current").output() {
        let output_str = String::from_utf8_lossy(&output.stdout);
        for line in output_str.lines() {
            if line.contains("primary") || line.contains("*") {
                info!("Screen info: {}", line.trim());
            }
        }
    }
    info!("Starting dioxus_bar v{}", 1.0);
    instance_name = format!("{}.{}", instance_name, instance_name);
    info!("instance_name: {instance_name}");
    let event_loop = EventLoopBuilder::with_user_event()
        .with_app_id(instance_name)
        .build();

    dioxus::LaunchBuilder::desktop()
        .with_cfg(
            Config::new()
                .with_window(
                    WindowBuilder::new()
                        .with_title("dioxus_bar")
                        .with_position(LogicalPosition::new(0, 0))
                        .with_maximizable(false)
                        .with_minimizable(false)
                        .with_resizable(true)
                        .with_always_on_top(true)
                        .with_visible_on_all_workspaces(true)
                        .with_decorations(false),
                )
                .with_event_loop(event_loop),
        )
        .launch(App);
}

// 将按钮数据定义为静态常量
const BUTTONS: &[&str] = &["🐖", "🐄", "🐂", "🐃", "🦥", "🦣", "🐏", "🦆", "🐢"];

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

// 截图按钮组件
#[component]
fn ScreenshotButton() -> Element {
    let mut is_taking_screenshot = use_signal(|| false);

    let take_screenshot = move |_| {
        if is_taking_screenshot() {
            return; // 防止重复点击
        }

        is_taking_screenshot.set(true);
        info!("Taking screenshot with flameshot");

        // 在新线程中执行截图命令，避免阻塞UI
        spawn(async move {
            let result =
                tokio::task::spawn_blocking(|| Command::new("flameshot").arg("gui").spawn()).await;

            match result {
                Ok(Ok(mut child)) => {
                    info!("Flameshot launched successfully");
                    // 等待命令完成
                    tokio::task::spawn_blocking(move || {
                        let _ = child.wait();
                    })
                    .await
                    .ok();
                }
                Ok(Err(e)) => {
                    error!("Failed to launch flameshot: {}", e);
                }
                Err(e) => {
                    error!("Task error when launching flameshot: {}", e);
                }
            }

            is_taking_screenshot.set(false);
        });
    };

    let button_class = if is_taking_screenshot() {
        "screenshot-button taking"
    } else {
        "screenshot-button"
    };

    rsx! {
        button {
            class: "{button_class}",
            onclick: take_screenshot,
            title: "截图 (Flameshot)",
            disabled: is_taking_screenshot(),

            span {
                class: "screenshot-icon",
                if is_taking_screenshot() {
                    "⏳" // 执行中
                } else {
                    "📷" // 默认截图图标
                }
            }
        }
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

    let time_format = if show_seconds {
        "%Y-%m-%d %H:%M:%S"
    } else {
        "%Y-%m-%d %H:%M"
    };
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
    let shared_path = std::env::args().nth(1).unwrap_or_else(|| {
        std::env::var("SHARED_MEMORY_PATH").unwrap_or_else(|_| "/dev/shm/monitor_0".to_string())
    });

    // 按钮状态数组
    let mut button_states = use_signal(|| vec![ButtonStateData::default(); BUTTONS.len()]);
    let mut last_update = use_signal(|| Instant::now());
    let mut show_seconds = use_signal(|| true);
    let mut system_snapshot = use_signal(|| None::<SystemSnapshot>);
    let mut pressed_button = use_signal(|| None::<usize>);
    let mut monitor_num = use_signal(|| None::<i32>);
    let mut layout_symbol = use_signal(|| " ? ".to_string());
    let mut shared_buffer_sig = use_signal(|| None::<SharedRingBuffer>);
    let shared_buffer_opt = SharedRingBuffer::create_shared_ring_buffer(&shared_path);
    if shared_buffer_opt.is_some() {
        shared_buffer_sig.set(shared_buffer_opt);
    }

    // 系统信息监控（保持原有逻辑）
    use_effect(move || {
        spawn(async move {
            let (sys_sender, sys_receiver) = std::sync::mpsc::channel();

            thread::spawn(move || {
                let mut monitor = SystemMonitor::new(30);
                monitor.set_update_interval(Duration::from_millis(2000));

                loop {
                    monitor.update_if_needed();
                    if let Some(snapshot) = monitor.get_snapshot() {
                        if sys_sender.send(snapshot.clone()).is_err() {
                            break;
                        }
                    }
                    thread::sleep(Duration::from_millis(500));
                }
            });

            loop {
                if let Ok(snapshot) = sys_receiver.try_recv() {
                    system_snapshot.set(Some(snapshot));
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });
    });

    // 共享内存通信逻辑
    use_effect(move || {
        let (message_sender, message_receiver) = mpsc::channel::<SharedMessage>();
        info!("Using shared memory path: {}", shared_path);
        let shared_path_clone = shared_path.clone();
        thread::spawn(move || {
            shared_memory_worker(shared_path_clone, message_sender);
        });

        spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(20));
            // 获取窗口控制句柄
            let window = use_window();
            let scale_factor = window.scale_factor();
            let mut prev_show_bar = true;
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
                    if now.duration_since(last_update()) >= Duration::from_millis(20) {
                        let mut new_states = vec![ButtonStateData::default(); BUTTONS.len()];
                        let monitor_info = shared_message.monitor_info;
                        layout_symbol.set(
                            monitor_info.get_ltsymbol()
                                + format!(" s: {:.2}", scale_factor).as_str()
                                + format!(", m: {}", monitor_info.monitor_num).as_str(),
                        );
                        monitor_num.set(Some(monitor_info.monitor_num));

                        // 更新按钮状态
                        let mut current_active_index = 0;
                        for (index, tag_status) in monitor_info.tag_status_vec.iter().enumerate() {
                            if index < new_states.len() {
                                new_states[index] = ButtonStateData {
                                    is_filtered: tag_status.is_filled,
                                    is_selected: tag_status.is_selected,
                                    is_urg: tag_status.is_urg,
                                    is_occ: tag_status.is_occ,
                                };
                                if tag_status.is_filled {
                                    current_active_index = index;
                                }
                            }
                        }
                        let show_bar = *monitor_info
                            .show_bars
                            .get(current_active_index)
                            .unwrap_or(&true);

                        // 调整前状态
                        let before_size = window.inner_size();
                        let before_pos = window.outer_position().unwrap_or_default();
                        let target_x = monitor_info.monitor_x;
                        let target_y = monitor_info.monitor_y;
                        let target_width = monitor_info.monitor_width;
                        let target_height: i32 = 42;
                        // 检查是否需要调整窗口
                        if (before_pos.x - target_x).abs() > 100
                            || (before_pos.y - target_y).abs() > 100
                            || (before_size.width as i32 - monitor_info.monitor_width).abs() > 10
                            || (before_size.height as i32 - target_height).abs() > 10
                            || (prev_show_bar != show_bar)
                        {
                            info!(
                                "Before: size={}x{}, pos=({}, {})",
                                before_size.width, before_size.height, before_pos.x, before_pos.y
                            );
                            info!(
                                "Target: size={}x{}, pos=({}, {})",
                                target_width, target_height, target_x, target_y
                            );
                            window.set_outer_position(LogicalPosition::new(
                                target_x as f64,
                                target_y as f64 + if show_bar { 0.0 } else { -1000.0 },
                            ));
                            window.set_inner_size(LogicalSize::new(
                                target_width as f64,
                                target_height as f64,
                            ));
                            // 在窗口调整代码中添加更详细的调试信息
                            info!("=== Window Adjustment Debug ===");
                            info!("Window decorations: {}", window.is_decorated());
                            info!("Window resizable: {}", window.is_resizable());
                            info!("Window maximized: {}", window.is_maximized());
                            info!("Window minimized: {}", window.is_minimized());
                            info!("Scale factor: {}", window.scale_factor());
                        }
                        prev_show_bar = show_bar;

                        let need_update_button_states = { *button_states.read() != new_states };
                        if need_update_button_states {
                            button_states.set(new_states);
                            last_update.set(now);
                        }
                    }
                }
            }
        });
    });

    // 按钮处理函数
    let mut handle_button_press = move |index: usize| {
        info!("Button {} pressed", index);
        pressed_button.set(Some(index));
    };

    let mut handle_button_release = move |index: usize| {
        info!("Button {} released", index);
        pressed_button.set(None);
        if let Some(monitor_num) = monitor_num() {
            let shared_buffer_opt = shared_buffer_sig.read();
            send_tag_command(&shared_buffer_opt, monitor_num, index, true);
        }
    };

    let mut handle_button_leave = move |_index: usize| {
        pressed_button.set(None);
    };

    rsx! {
        document::Style { "{STYLE_CSS}" }

        div {
            class: "button-row",

            div {
                class: "buttons-container",
                for (i, emoji) in BUTTONS.iter().enumerate() {
                    {
                        let base_class = get_button_class(i, &button_states());
                        let is_pressed = pressed_button() == Some(i);
                        let button_class = if is_pressed {
                            format!("{} pressed", base_class)
                        } else {
                            base_class.to_string()
                        };

                        rsx! {
                            button {
                                key: "{i}",
                                class: "{button_class}",
                                onmousedown: move |_| handle_button_press(i),
                                onmouseup: move |_| handle_button_release(i),
                                onmouseleave: move |_| handle_button_leave(i),
                                onclick: move |_| {
                                    if pressed_button() == Some(i) {
                                        handle_button_release(i);
                                    }
                                },
                                "{emoji}"
                            }
                        }
                    }
                }

                // 添加 layout_symbol 显示
                span {
                    class: "layout-symbol",
                    title: "当前布局",
                    "{layout_symbol()}"
                }

            }

            div {
                class: "right-info-container",
                SystemInfoDisplay { snapshot: system_snapshot() }
                ScreenshotButton {}
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
