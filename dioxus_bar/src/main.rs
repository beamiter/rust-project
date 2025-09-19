use chrono::Local;
use dioxus::{
    desktop::{Config, LogicalPosition, WindowBuilder, use_window},
    prelude::*,
};
use log::{debug, error, info, warn};
use std::{
    env,
    process::Command,
    sync::Arc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use shared_structures::{CommandType, SharedCommand, SharedMessage, SharedRingBuffer};
use xbar_core::initialize_logging;
use xbar_core::system_monitor::SystemMonitor;
use xbar_core::system_monitor::SystemSnapshot;

// 在编译时直接包含CSS文件
const STYLE_CSS: &str = include_str!("../assets/style.css");

// 优化的共享内存工作线程 - 降低CPU使用率
fn shared_memory_worker(
    shared_buffer_opt: Option<Arc<SharedRingBuffer>>,
    message_sender: tokio::sync::mpsc::Sender<SharedMessage>,
) {
    info!("Starting shared memory worker thread");
    let mut prev_timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    if let Some(shared_buffer) = shared_buffer_opt {
        loop {
            match shared_buffer.wait_for_message(Some(std::time::Duration::from_secs(2))) {
                Ok(true) => {
                    if let Ok(Some(message)) = shared_buffer.try_read_latest_message() {
                        if prev_timestamp != message.timestamp.into() {
                            prev_timestamp = message.timestamp.into();
                            if let Err(e) = message_sender.blocking_send(message) {
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
    shared_buffer: &SharedRingBuffer,
    monitor_id: i32,
    active_tab: usize,
    is_view: bool,
) {
    let tag_bit = 1 << active_tab;
    the_cmd_send(
        shared_buffer,
        if is_view {
            SharedCommand::view_tag(tag_bit, monitor_id)
        } else {
            SharedCommand::toggle_tag(tag_bit, monitor_id)
        },
    );
}

fn the_cmd_send(shared_buffer: &SharedRingBuffer, cmd: SharedCommand) {
    match shared_buffer.send_command(cmd) {
        Ok(true) => info!("Sent command: by shared_buffer"),
        Ok(false) => warn!("Command buffer full, command dropped"),
        Err(e) => error!("Failed to send command: {}", e),
    }
}

fn send_layout_command(shared_buffer: &SharedRingBuffer, monitor_id: i32, layout_index: u32) {
    let cmd = SharedCommand::new(CommandType::SetLayout, layout_index, monitor_id);
    the_cmd_send(shared_buffer, cmd);
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

// 截图按钮组件（Pill）
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
                    let _ = tokio::task::spawn_blocking(move || child.wait()).await;
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

    rsx! {
        div {
            class: "pill screenshot-pill",
            onclick: take_screenshot,
            title: "截图 (Flameshot)",
            { if is_taking_screenshot() { "⏳" } else { "📸" } }
        }
    }
}

/// 系统信息显示组件（Pill）
#[component]
fn SystemInfoDisplay(snapshot: Option<SystemSnapshot>) -> Element {
    // 阈值 -> class
    let sev = |p: f32| {
        if p <= 30.0 {
            "usage-good"
        } else if p <= 60.0 {
            "usage-warn"
        } else if p <= 80.0 {
            "usage-caution"
        } else {
            "usage-danger"
        }
    };

    if let Some(ref s) = snapshot {
        let cpu_class = sev(s.cpu_average as f32);
        let mem_class = sev(s.memory_usage_percent as f32);

        // 电池按高->低好到差
        let batt_class = if s.battery_percent > 50.0 {
            "usage-good"
        } else if s.battery_percent > 20.0 {
            "usage-warn"
        } else {
            "usage-danger"
        };

        let cpu_cls = format!("pill usage-pill {}", cpu_class);
        let mem_cls = format!("pill usage-pill {}", mem_class);
        let batt_cls = format!("pill usage-pill {}", batt_class);

        let mem_title = format!(
            "内存使用: {} / {}",
            format_bytes(s.memory_used),
            format_bytes(s.memory_total)
        );
        let batt_title = if s.is_charging {
            format!("电池充电中: {:.1}%", s.battery_percent)
        } else {
            format!("电池电量: {:.1}%", s.battery_percent)
        };
        let batt_icon = if s.is_charging { "🔌" } else { "🔋" };

        rsx! {
            div { class: "system-info-container",
                div { class: "{cpu_cls}", title: "CPU 平均使用率",
                    {format!("CPU {:.0}%", s.cpu_average)}
                }
                div { class: "{mem_cls}", title: "{mem_title}",
                    {format!("MEM {:.0}%", s.memory_usage_percent)}
                }
                div { class: "{batt_cls}", title: "{batt_title}",
                    {format!("{} {:.0}%", batt_icon, s.battery_percent)}
                }
            }
        }
    } else {
        rsx! {
            div { class: "system-info-container",
                div { class: "pill usage-pill usage-warn", "CPU --%" }
                div { class: "pill usage-pill usage-warn", "MEM --%" }
                div { class: "pill usage-pill usage-warn", "🔋 --%" }
            }
        }
    }
}

/// 时间文本组件（只输出文本）
#[component]
fn TimeText(show_seconds: bool) -> Element {
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

    rsx! { span { "{time_str}" } }
}

// 将按钮数据定义为静态常量（可改为你的动物 emoji，以保持样式不变，这里用更语义化的）
const BUTTONS: &[&str] = &["🏠", "💻", "🌐", "🎵", "📁", "🎮", "📧", "🔧", "📊"];

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

fn main() {
    let args: Vec<String> = env::args().collect();
    let shared_path = args.get(1).cloned().unwrap_or_default();
    if let Err(e) = initialize_logging("dioxus_bar", &shared_path) {
        error!("Failed to initialize logging: {}", e);
        std::process::exit(1);
    }
    // 环境信息
    info!("=== Environment Debug Info ===");
    info!("DISPLAY: {:?}", env::var("DISPLAY"));
    info!("WAYLAND_DISPLAY: {:?}", env::var("WAYLAND_DISPLAY"));
    info!("XDG_SESSION_TYPE: {:?}", env::var("XDG_SESSION_TYPE"));
    info!("DESKTOP_SESSION: {:?}", env::var("DESKTOP_SESSION"));
    info!("XDG_CURRENT_DESKTOP: {:?}", env::var("XDG_CURRENT_DESKTOP"));
    // 屏幕分辨率（如果可能）
    if let Ok(output) = Command::new("xrandr").arg("--current").output() {
        let output_str = String::from_utf8_lossy(&output.stdout);
        for line in output_str.lines() {
            if line.contains("primary") || line.contains("*") {
                info!("Screen info: {}", line.trim());
            }
        }
    }
    info!("Starting dioxus_bar v{}", 1.0);

    dioxus::LaunchBuilder::desktop()
        .with_cfg(
            Config::new().with_window(
                WindowBuilder::new()
                    .with_title("dioxus_bar")
                    .with_position(LogicalPosition::new(0, 0))
                    .with_maximizable(false)
                    .with_minimizable(false)
                    .with_resizable(true)
                    .with_always_on_top(true)
                    .with_visible_on_all_workspaces(true)
                    .with_decorations(false),
            ),
        )
        .launch(App);
}

#[component]
fn App() -> Element {
    let shared_path = std::env::args().nth(1).unwrap_or_else(|| {
        std::env::var("SHARED_MEMORY_PATH").unwrap_or_else(|_| "/dev/shm/monitor_0".to_string())
    });

    // 窗口/缩放因子
    let window = use_window();
    let scale_factor = use_signal(|| window.scale_factor());

    // 按钮状态数组
    let mut button_states = use_signal(|| vec![ButtonStateData::default(); BUTTONS.len()]);
    let mut last_update = use_signal(|| Instant::now());
    let mut show_seconds = use_signal(|| true);
    let mut system_snapshot = use_signal(|| None::<SystemSnapshot>);
    let mut pressed_button = use_signal(|| None::<usize>);
    let mut monitor_num = use_signal(|| None::<i32>);
    let mut layout_symbol = use_signal(|| "[]=".to_string());
    let mut layout_open = use_signal(|| false);

    let shared_buffer_sig = use_signal(|| {
        info!(
            "(SIGNAL-INIT) Creating shared ring buffer for path: {}",
            shared_path
        );
        SharedRingBuffer::create_shared_ring_buffer_aux(&shared_path).map(Arc::new)
    });

    // 监视器图标
    let monitor_icon = |num: i32| -> &'static str {
        match num {
            0 => "󰎡",
            1 => "󰎤",
            _ => "?",
        }
    };

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
        let (message_sender, mut message_receiver) =
            tokio::sync::mpsc::channel::<SharedMessage>(100);
        info!("Using shared memory path: {}", shared_path);
        let shared_buffer_for_worker = shared_buffer_sig.read().clone();
        thread::spawn(move || {
            shared_memory_worker(shared_buffer_for_worker, message_sender);
        });

        spawn(async move {
            // 异步等待消息，无需轮询
            while let Some(shared_message) = message_receiver.recv().await {
                let mut new_states = vec![ButtonStateData::default(); BUTTONS.len()];
                let monitor_info = shared_message.monitor_info;

                layout_symbol.set(monitor_info.get_ltsymbol());
                monitor_num.set(Some(monitor_info.monitor_num));

                // 更新按钮状态
                for (index, tag_status) in monitor_info.tag_status_vec.iter().enumerate() {
                    if index < new_states.len() {
                        new_states[index] = ButtonStateData {
                            is_filtered: tag_status.is_filled,
                            is_selected: tag_status.is_selected,
                            is_urg: tag_status.is_urg,
                            is_occ: tag_status.is_occ,
                        };
                    }
                }
                let need_update_button_states = { *button_states.read() != new_states };
                if need_update_button_states {
                    button_states.set(new_states);
                    last_update.set(Instant::now());
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
        if let (Some(monitor_id), Some(buffer_arc)) =
            (monitor_num(), shared_buffer_sig.read().as_ref())
        {
            send_tag_command(buffer_arc, monitor_id, index, true);
        } else {
            warn!("Shared buffer or monitor_num not available, cannot send command.");
        }
    };

    let mut handle_button_leave = move |_index: usize| {
        pressed_button.set(None);
    };

    // 布局面板控制
    let toggle_layout_panel = move |_| {
        layout_open.set(!layout_open());
    };

    let mut select_layout = move |idx: u32| {
        layout_open.set(false);
        if let (Some(monitor_id), Some(buffer_arc)) =
            (monitor_num(), shared_buffer_sig.read().as_ref())
        {
            send_layout_command(buffer_arc, monitor_id, idx);
        } else {
            warn!("Shared buffer or monitor_num not available for layout set.");
        }
    };

    rsx! {
        document::Style { "{STYLE_CSS}" }

        div { class: "button-row",

            div { class: "buttons-container",
                // 工作区按钮（Tag）
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

                // 布局切换 + 选项（Pill）
                div { class: "layout-controls",
                    // 开关按钮
                    {
                        let toggle_class = format!(
                            "pill layout-toggle {}",
                            if layout_open() { "open" } else { "closed" }
                        );
                        rsx! {
                            div {
                                class: "{toggle_class}",
                                onclick: toggle_layout_panel,
                                "{layout_symbol()}"
                            }
                        }
                    }

                    // 选项行（展开时）
                    if layout_open() {
                        {
                            let current = layout_symbol();
                            let lo0 = format!("pill layout-option {}", if current.contains("[]=") { "current" } else { "" });
                            let lo1 = format!("pill layout-option {}", if current.contains("><>") { "current" } else { "" });
                            let lo2 = format!("pill layout-option {}", if current.contains("[M]") { "current" } else { "" });

                            rsx! {
                                div { class: "layout-selector",
                                    div {
                                        class: "{lo0}",
                                        onclick: move |_| select_layout(0),
                                        "[]="
                                    }
                                    div {
                                        class: "{lo1}",
                                        onclick: move |_| select_layout(1),
                                        "><>"
                                    }
                                    div {
                                        class: "{lo2}",
                                        onclick: move |_| select_layout(2),
                                        "[M]"
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // 中间撑开
            div { class: "spacer" }

            // 右侧信息（Pill）
            div { class: "right-info-container",
                SystemInfoDisplay { snapshot: system_snapshot() }
                ScreenshotButton {}

                // 时间 pill（点击切换秒显示）
                div {
                    class: "pill time-pill",
                    onclick: move |_| {
                        show_seconds.set(!show_seconds());
                        info!("Toggle seconds display: {}", show_seconds());
                    },
                    TimeText { show_seconds: show_seconds() }
                }

                // Monitor 指示 pill
                div { class: "pill monitor-pill",
                    {format!("🖥️ {}", monitor_icon(monitor_num().unwrap_or(0)))}
                }

                // Scale factor pill
                div { class: "pill scale-pill",
                    {format!("s: {:.2}", scale_factor())}
                }
            }
        }
    }
}
