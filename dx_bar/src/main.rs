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
    time::{Duration, SystemTime, UNIX_EPOCH},
};
mod error;
pub use error::AppError;

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

fn shared_memory_worker(
    shared_path: String,
    message_sender: mpsc::Sender<SharedMessage>,
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

fn main() {
    let args: Vec<String> = env::args().collect();
    let class_instance = args.get(0).cloned().unwrap_or_default();
    let shared_path = args.get(1).cloned().unwrap_or_default();

    // Initialize logging
    if let Err(e) = initialize_logging(&shared_path) {
        error!("Failed to initialize logging: {}", e);
        std::process::exit(1);
    }

    info!("Starting dx_bar v{}", 1.0);

    dioxus::LaunchBuilder::desktop()
        .with_cfg(Config::new().with_window(WindowBuilder::new().with_title("dx_bar")))
        .launch(App);
}

// 将按钮数据定义为静态常量
const BUTTONS: &[&str] = &["🔴", "🟠", "🟡", "🟢", "🔵", "🟣", "🟤", "⚪", "⚫", "🌈"];

#[component]
fn App() -> Element {
    // UI 状态
    let mut message = use_signal(|| "请选择一个按钮".to_string());
    let mut selected_button = use_signal(|| None::<usize>);
    let mut click_count = use_signal(|| 0u32);

    // 共享内存同步状态
    let mut external_selected_button = use_signal(|| None::<usize>);
    let mut connection_status = use_signal(|| "未连接".to_string());

    // 初始化共享内存通信
    use_effect(move || {
        let (message_sender, message_receiver) = mpsc::channel::<SharedMessage>();
        let (command_sender, command_receiver) = mpsc::channel::<SharedCommand>();

        // 你可以根据需要配置共享内存路径
        let shared_path = std::env::var("SHARED_MEMORY_PATH")
            .unwrap_or_else(|_| "/dev/shm/monitor_0".to_string());

        // 启动共享内存工作线程
        let shared_path_clone = shared_path.clone();
        thread::spawn(move || {
            shared_memory_worker(shared_path_clone, message_sender, command_receiver);
        });

        // 启动消息接收线程
        spawn(async move {
            loop {
                if let Ok(shared_message) = message_receiver.try_recv() {
                    info!("Received shared message: {:?}", shared_message);
                    let mut button_index: Option<usize> = None;
                    for (index, tag_status) in shared_message
                        .monitor_info
                        .tag_status_vec
                        .iter()
                        .enumerate()
                    {
                        if tag_status.is_selected {
                            button_index = Some(index);
                            break;
                        }
                    }
                    // 更新外部选择状态
                    if let Some(index) = button_index {
                        if index < BUTTONS.len() {
                            external_selected_button.set(Some(index));
                            message.set(format!("外部选择: {}", BUTTONS[index]));
                            connection_status.set("已连接 - 接收数据".to_string());
                        }
                    } else {
                        external_selected_button.set(None);
                        message.set("外部清除选择".to_string());
                    }
                }

                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });

        // 保存 command_sender 以便发送命令
        // 注意：这里需要使用某种方式保存 command_sender，比如使用 use_context
        // 或者将其存储在一个全局状态中
    });

    // 合并本地选择和外部选择
    let current_selection = if external_selected_button().is_some() {
        external_selected_button()
    } else {
        selected_button()
    };

    rsx! {
        document::Link {
            rel: "stylesheet",
            href: asset!("./assets/style.css"),
        }

        div {
            class: "app-container",

            h2 {
                class: "app-title",
                "Emoji 按钮选择器 (共享内存同步)"
            }

            // 连接状态显示
            div {
                class: "connection-status",
                style: "margin-bottom: 15px; padding: 8px; border-radius: 4px; background: #e9ecef;",
                "连接状态: {connection_status()}"
            }

            div {
                class: "button-container",
                for (i, emoji) in BUTTONS.iter().enumerate() {
                    button {
                        key: "{i}",
                        class: if current_selection == Some(i) {
                            "emoji-button selected"
                        } else {
                            "emoji-button"
                        },
                        onclick: move |_| {
                            // 只有在没有外部选择时才允许本地选择
                            if external_selected_button().is_none() {
                                selected_button.set(Some(i));
                                message.set(format!("本地选择: {}", emoji));
                                click_count.set(click_count() + 1);

                                // TODO: 发送命令到共享内存
                                // command_sender.send(SharedCommand::SelectButton(i));
                            }
                        },
                        // 当有外部选择时，禁用本地点击
                        disabled: external_selected_button().is_some(),
                        "{emoji}"
                    }
                }
            }

            p {
                class: "message-display",
                "{message()}"
            }

            div {
                class: "status-info",

                div {
                    class: "status-title",
                    "选择状态:"
                }

                div {
                    class: "current-selection",
                    if let Some(index) = current_selection {
                        if external_selected_button().is_some() {
                            "外部选择: {BUTTONS[index]} (索引: {index})"
                        } else {
                            "本地选择: {BUTTONS[index]} (索引: {index})"
                        }
                    } else {
                        "暂无选择"
                    }
                }

                div {
                    class: "selection-count",
                    "本地点击次数: {click_count()}"
                }

                // 只有在没有外部选择时才显示清除按钮
                if external_selected_button().is_none() {
                    button {
                        class: "clear-button",
                        disabled: selected_button().is_none(),
                        onclick: move |_| {
                            selected_button.set(None);
                            message.set("已清除本地选择".to_string());

                            // TODO: 发送清除命令到共享内存
                            // command_sender.send(SharedCommand::ClearSelection);
                        },
                        "清除选择"
                    }
                }

                // 强制清除外部选择的按钮（调试用）
                if external_selected_button().is_some() {
                    button {
                        class: "clear-button",
                        style: "background: #ffc107; color: #000;",
                        onclick: move |_| {
                            external_selected_button.set(None);
                            message.set("强制清除外部选择".to_string());
                        },
                        "强制清除外部选择"
                    }
                }
            }
        }
    }
}
