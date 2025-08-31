// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use chrono::Local;
use flexi_logger::{Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming};
use log::{error, info, warn};
use shared_structures::TagStatus;
use tauri::Emitter;
use tauri::Manager;

use std::{env, process::Command as StdCommand, sync::mpsc, thread, time::Duration};

// 引入我们的模块
mod error;
use error::AppError;
mod system_monitor;
use shared_structures::{MonitorInfo, SharedCommand, SharedMessage, SharedRingBuffer};
use system_monitor::{SystemMonitor, SystemSnapshot};

#[derive(Clone, Debug, serde::Serialize)]
pub struct MonitorInfoSnapshot {
    pub monitor_num: i32,
    pub monitor_width: i32,
    pub monitor_height: i32,
    pub monitor_x: i32,
    pub monitor_y: i32,
    pub tag_status_vec: Vec<TagStatus>,
    pub client_name: String,
    pub ltsymbol: String,
}
impl MonitorInfoSnapshot {
    pub fn new(monitor_info: &MonitorInfo) -> Self {
        Self {
            monitor_num: monitor_info.monitor_num,
            monitor_width: monitor_info.monitor_width,
            monitor_height: monitor_info.monitor_height,
            monitor_x: monitor_info.monitor_x,
            monitor_y: monitor_info.monitor_y,
            tag_status_vec: monitor_info.tag_status_vec.to_vec(),
            client_name: monitor_info.get_client_name(),
            ltsymbol: monitor_info.get_ltsymbol(),
        }
    }
}
// 定义一个整合所有UI状态的结构体，方便序列化为JSON
#[derive(Clone, serde::Serialize)]
struct UiState {
    monitor_info_snapshot: MonitorInfoSnapshot,
    system_snapshot: Option<SystemSnapshot>,
}

// 应用状态，用于在Tauri命令间共享
struct AppState {
    command_sender: mpsc::Sender<SharedCommand>,
}

/// 初始化日志系统 (与原版相同)
fn initialize_logging(shared_path: &str) -> Result<(), AppError> {
    let now = Local::now();
    let timestamp = now.format("%Y-%m-%d_%H_%M_%S").to_string();

    let file_name = if shared_path.is_empty() {
        "tauri_bar".to_string()
    } else {
        std::path::Path::new(shared_path)
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| format!("tauri_bar_{}", name))
            .unwrap_or_else(|| "tauri_bar".to_string())
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
    // let mut last_update_time = Instant::now();

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
            match sb.try_read_latest_message() {
                Ok(Some(msg)) => {
                    if last_message
                        .as_ref()
                        .map_or(true, |m| m.timestamp != msg.timestamp)
                    {
                        info!("msg: {:?}", msg);
                        last_message = Some(msg);
                        state_changed = true;
                    }
                }
                Ok(_) => (),
                Err(e) => error!("Error reading from shared buffer: {}", e),
            }
        }

        // 3. 从系统监视器线程接收最新的快照
        if let Ok(snapshot) = sys_receiver.try_recv() {
            last_snapshot = Some(snapshot);
            state_changed = true;
        }

        // 4. 如果状态有变或达到更新间隔，则向前端发送事件
        if state_changed {
            if let Some(msg) = &last_message {
                let window = app_handle.get_webview_window("main").unwrap();
                let monitor_info = &msg.monitor_info;
                let mut monitor_info_snapshot = MonitorInfoSnapshot::new(&monitor_info);
                let scale_factor = window.scale_factor().unwrap();
                let new_symbol = monitor_info.get_ltsymbol()
                    + format!(" s: {:.2}", scale_factor).as_str()
                    + format!(", m: {}", monitor_info.monitor_num).as_str();
                monitor_info_snapshot.ltsymbol = new_symbol;
                info!("monitor_info_snapshot: {:?}", monitor_info_snapshot);
                let state = UiState {
                    monitor_info_snapshot,
                    system_snapshot: last_snapshot.clone(),
                };
                info!("Validating state before emit:");
                info!("- monitor_num: {}", state.monitor_info_snapshot.monitor_num);
                info!(
                    "- tag_status_vec length: {}",
                    state.monitor_info_snapshot.tag_status_vec.len()
                );
                info!(
                    "- client_name: '{}'",
                    state.monitor_info_snapshot.client_name
                );
                match app_handle.emit("state-update", &state) {
                    Ok(_) => {
                        info!("✅ Successfully emitted state-update event");
                    }
                    Err(e) => {
                        error!("❌ Failed to emit state-update event: {}", e);
                    }
                }
                // last_update_time = Instant::now();
            }
        }

        thread::sleep(Duration::from_millis(20));
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let shared_path = args.get(1).cloned().unwrap_or_default();
    if let Err(e) = initialize_logging(&shared_path) {
        eprintln!("Failed to initialize logging: {}", e);
    }

    let mut instance_name = shared_path.replace("/dev/shm/monitor_", "tauri_bar_");
    if instance_name.is_empty() {
        instance_name = "tauri_bar".to_string();
    }
    let mut context = tauri::generate_context!();
    context.config_mut().product_name = Some(instance_name.clone());
    info!("product_name: {:?}", context.config_mut().product_name);
    instance_name = format!("{}.{}", instance_name, instance_name);
    info!("instance_name: {}", instance_name);
    context.config_mut().identifier = instance_name;

    info!("=== Environment Debug Info ===");
    let shared_path_clone = shared_path.clone();
    tauri::Builder::new()
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            // 在 setup hook 中启动我们的后台工作线程
            let app_handle = app.handle().clone();
            let app_id = app_handle.config().identifier.clone();
            info!("Application ID has been set to: {}", app_id);
            thread::spawn(move || {
                background_worker(app_handle, shared_path_clone);
            });

            // let window = app.get_webview_window("main").unwrap();
            // window.open_devtools();

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![send_tag_command, take_screenshot])
        .run(context)
        .expect("error while running tauri application");
}
