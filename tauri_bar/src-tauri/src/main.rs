// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use chrono::Local;
use flexi_logger::{Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming};
use log::{error, info, warn};
use tauri::Emitter;
use tauri::Manager;

use std::{
    env,
    process::Command as StdCommand,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

// 引入我们的模块
mod error;
use error::AppError;
mod system_monitor;
use shared_structures::{MonitorInfo, SharedCommand, SharedMessage, SharedRingBuffer};
use system_monitor::{SystemMonitor, SystemSnapshot};

// 定义一个整合所有UI状态的结构体，方便序列化为JSON
#[derive(Clone, serde::Serialize)]
struct UiState {
    monitor_info: MonitorInfo,
    system_snapshot: Option<SystemSnapshot>,
}

// 应用状态，用于在Tauri命令间共享
struct AppState {
    command_sender: mpsc::Sender<SharedCommand>,
}

/// 初始化日志系统 (与原版相同)
fn initialize_logging(shared_path: &str) -> Result<(), AppError> {
    // ... (此处代码与你的原版 `initialize_logging` 完全相同)
    let now = Local::now();
    let timestamp = now.format("%Y-%m-%d_%H_%M_%S").to_string();

    let file_name = if shared_path.is_empty() {
        "dx_bar_tauri".to_string()
    } else {
        std::path::Path::new(shared_path)
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| format!("dx_bar_tauri_{}", name))
            .unwrap_or_else(|| "dx_bar_tauri".to_string())
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

/// Tauri 命令：发送标签操作
#[tauri::command]
fn send_tag_command(
    tag_index: usize,
    is_view: bool,
    monitor_id: i32,
    state: tauri::State<'_, AppState>,
) {
    let tag_bit = 1 << tag_index;
    let command = if is_view {
        SharedCommand::view_tag(tag_bit, monitor_id)
    } else {
        SharedCommand::toggle_tag(tag_bit, monitor_id)
    };

    match state.command_sender.send(command) {
        Ok(_) => {
            let action = if is_view { "ViewTag" } else { "ToggleTag" };
            info!("Sent {} command for tag {}", action, tag_index + 1);
        }
        Err(e) => {
            let action = if is_view { "ViewTag" } else { "ToggleTag" };
            error!("Failed to send {} command: {}", action, e);
        }
    }
}

/// Tauri 命令：执行截图
#[tauri::command]
async fn take_screenshot() -> Result<(), String> {
    info!("Taking screenshot with flameshot");
    StdCommand::new("flameshot")
        .arg("gui")
        .spawn()
        .map_err(|e| format!("Failed to launch flameshot: {}", e))?;
    Ok(())
}

// 这是重构后的核心工作线程，整合了共享内存和系统监控的数据
fn background_worker(app_handle: tauri::AppHandle, shared_path: String) {
    info!("Starting background worker thread");

    // 设置命令通道
    let (command_sender, command_receiver) = mpsc::channel::<SharedCommand>();
    app_handle.manage(AppState { command_sender });

    // 初始化共享内存
    let shared_buffer_opt: Option<SharedRingBuffer> = if shared_path.is_empty() {
        warn!("No shared path provided, running without shared memory");
        None
    } else {
        // ... (此处打开/创建共享内存的逻辑与你的 `shared_memory_worker` 相同)
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

    // 初始化系统监视器
    let (sys_sender, sys_receiver) = mpsc::channel::<SystemSnapshot>();
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

    let mut last_message: Option<SharedMessage> = None;
    let mut last_snapshot: Option<SystemSnapshot> = None;
    let mut last_update_time = Instant::now();

    // 主循环
    loop {
        let mut state_changed = false;

        // 1. 处理来自前端的命令，并发送到共享内存
        while let Ok(cmd) = command_receiver.try_recv() {
            info!("Received command from frontend: {:?}", cmd);
            if let Some(ref sb) = shared_buffer_opt {
                if let Err(e) = sb.send_command(cmd) {
                    error!("Failed to send command via shared buffer: {}", e);
                }
            }
        }

        // 2. 从共享内存读取最新的状态
        if let Some(ref sb) = shared_buffer_opt {
            match sb.try_read_latest_message::<SharedMessage>() {
                Ok(Some(msg)) => {
                    if last_message
                        .as_ref()
                        .map_or(true, |m| m.timestamp != msg.timestamp)
                    {
                        last_message = Some(msg);
                        state_changed = true;
                    }
                }
                Ok(None) => (),
                Err(e) => error!("Error reading from shared buffer: {}", e),
            }
        }

        // 3. 从系统监视器线程接收最新的快照
        if let Ok(snapshot) = sys_receiver.try_recv() {
            last_snapshot = Some(snapshot);
            state_changed = true;
        }

        // 4. 如果状态有变或达到更新间隔，则向前端发送事件
        if state_changed || last_update_time.elapsed() > Duration::from_millis(50) {
            if let Some(msg) = &last_message {
                // 窗口位置调整逻辑
                let window = app_handle.get_webview_window("main").unwrap();
                let monitor_info = &msg.monitor_info;
                window
                    .set_position(tauri::LogicalPosition::new(
                        monitor_info.monitor_x,
                        monitor_info.monitor_y,
                    ))
                    .unwrap();
                window
                    .set_size(tauri::LogicalSize::new(monitor_info.monitor_width, 50))
                    .unwrap();

                let state = UiState {
                    monitor_info: msg.monitor_info.clone(),
                    system_snapshot: last_snapshot.clone(),
                };
                app_handle.emit("state-update", state).unwrap();
                last_update_time = Instant::now();
            }
        }

        thread::sleep(Duration::from_millis(20));
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let shared_path = args.get(1).cloned().unwrap_or_default();
    if let Err(e) = initialize_logging(&shared_path) {
        // 在Tauri应用启动前，日志错误只能打印到stderr
        eprintln!("Failed to initialize logging: {}", e);
    }

    info!("=== Environment Debug Info ===");
    // ... (环境信息日志可以保留)

    let shared_path_clone = shared_path.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            // 在 setup hook 中启动我们的后台工作线程
            let app_handle = app.handle().clone();
            thread::spawn(move || {
                background_worker(app_handle, shared_path_clone);
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![send_tag_command, take_screenshot])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
