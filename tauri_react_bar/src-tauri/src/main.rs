// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use log::debug;
use log::{error, info, warn};
use tauri::Emitter;
use tauri::Manager;

use std::{env, sync::Arc, time::Duration};
use tokio::time::sleep;

use shared_structures::TagStatus;
use shared_structures::{CommandType, MonitorInfo, SharedCommand, SharedMessage, SharedRingBuffer};
use xbar_core::initialize_logging;
use xbar_core::system_monitor::SystemMonitor;
use xbar_core::system_monitor::SystemSnapshot;

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

// 应用状态，用于在Tauri命令间共享
struct AppState {
    shared_buffer: Option<Arc<SharedRingBuffer>>,
}

// 共享的应用状态，用于在不同任务之间共享数据
#[derive(Clone)]
struct SharedAppState {
    app_handle: tauri::AppHandle,
}

impl SharedAppState {
    fn new(app_handle: tauri::AppHandle) -> Self {
        Self { app_handle }
    }

    // 直接发送监视器信息更新
    async fn emit_monitor_update(
        &self,
        message: &SharedMessage,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let window = self
            .app_handle
            .get_webview_window("main")
            .ok_or("Main window not found")?;

        let monitor_info = &message.monitor_info;
        let mut monitor_info_snapshot = MonitorInfoSnapshot::new(monitor_info);

        let scale_factor = window.scale_factor()?;
        let new_symbol = format!(
            "{} s: {:.2}, m: {}",
            monitor_info.get_ltsymbol(),
            scale_factor,
            monitor_info.monitor_num
        );
        monitor_info_snapshot.ltsymbol = new_symbol;

        info!("Emitting monitor-update:");
        info!("- monitor_num: {}", monitor_info_snapshot.monitor_num);
        info!(
            "- tag_status_vec length: {}",
            monitor_info_snapshot.tag_status_vec.len()
        );
        info!("- client_name: '{}'", monitor_info_snapshot.client_name);

        self.app_handle
            .emit("monitor-update", &monitor_info_snapshot)?;
        Ok(())
    }

    // 直接发送系统信息更新
    async fn emit_system_update(
        &self,
        snapshot: &SystemSnapshot,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Emitting system-update");
        self.app_handle.emit("system-update", snapshot)?;
        Ok(())
    }
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

    if let Some(shared_buffer) = state.shared_buffer.as_ref() {
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

/// Tauri 命令：切换布局
#[tauri::command]
fn send_layout_command(layout_index: u32, monitor_id: i32, state: tauri::State<'_, AppState>) {
    let command = SharedCommand::new(CommandType::SetLayout, layout_index, monitor_id);
    if let Some(shared_buffer) = state.shared_buffer.as_ref() {
        match shared_buffer.send_command(command) {
            Ok(true) => info!("Sent layout command: {:?} by shared_buffer", command),
            Ok(false) => warn!("Command buffer full, command dropped"),
            Err(e) => error!("Failed to send layout command: {}", e),
        }
    }
}

/// Tauri 命令：执行截图
#[tauri::command]
async fn take_screenshot() -> Result<(), String> {
    info!("Taking screenshot with flameshot");

    tokio::process::Command::new("flameshot")
        .arg("gui")
        .spawn()
        .map_err(|e| format!("Failed to launch flameshot: {}", e))?;

    Ok(())
}

async fn system_monitor_task(shared_state: SharedAppState) {
    info!("Starting system monitor task");
    tokio::task::spawn_blocking(move || {
        let mut monitor = SystemMonitor::new(30);
        monitor.set_update_interval(Duration::from_millis(2000));
        let rt = tokio::runtime::Handle::current();
        loop {
            monitor.update_if_needed();
            if let Some(snapshot) = monitor.get_snapshot() {
                // 直接发送系统更新事件
                if let Err(e) = rt.block_on(shared_state.emit_system_update(&snapshot)) {
                    error!("Failed to emit system update: {}", e);
                }
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    });
}

async fn shared_memory_monitor_task(
    shared_state: SharedAppState,
    shared_buffer: Arc<SharedRingBuffer>,
) {
    info!("Starting shared memory monitor task");
    let mut last_timestamp: Option<u64> = None;
    loop {
        let buffer_clone = shared_buffer.clone();
        match buffer_clone.wait_for_message(Some(Duration::from_secs(2))) {
            Ok(true) => {
                if let Ok(Some(msg)) = shared_buffer.try_read_latest_message() {
                    if last_timestamp.map_or(true, |ts| ts != msg.timestamp) {
                        info!("Received new message with timestamp: {}", msg.timestamp);
                        last_timestamp = Some(msg.timestamp);
                        // 直接发送监视器更新事件
                        if let Err(e) = shared_state.emit_monitor_update(&msg).await {
                            error!("Failed to emit monitor update: {}", e);
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

/// 简化的后台工作协调器
async fn background_worker(app_handle: tauri::AppHandle, shared_path: String) {
    info!("Starting background worker coordinator");

    // 初始化共享内存
    let shared_arc = SharedRingBuffer::create_shared_ring_buffer_aux(&shared_path).map(Arc::new);

    // 设置应用状态用于命令处理
    app_handle.manage(AppState {
        shared_buffer: shared_arc.clone(),
    });

    // 创建共享应用状态
    let shared_state = SharedAppState::new(app_handle);

    // 启动系统监控任务
    system_monitor_task(shared_state.clone()).await;

    // 如果有共享内存，启动共享内存监控任务
    if let Some(shared_buffer) = shared_arc {
        shared_memory_monitor_task(shared_state.clone(), shared_buffer).await;
    } else {
        // 如果没有共享内存，保持主任务运行
        info!("No shared memory available, only system monitoring will be active");
        loop {
            sleep(Duration::from_secs(1)).await;
        }
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    let shared_path = args.get(1).cloned().unwrap_or_default();

    if let Err(e) = initialize_logging("tauri_react_bar", &shared_path) {
        eprintln!("Failed to initialize logging: {}", e);
    }

    info!("=== Environment Debug Info ===");
    let shared_path_clone = shared_path.clone();

    tauri::Builder::new()
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            let app_handle = app.handle().clone();
            let app_id = app_handle.config().identifier.clone();
            info!("Application ID has been set to: {}", app_id);

            // 启动后台工作协调器
            tokio::spawn(async move {
                background_worker(app_handle, shared_path_clone).await;
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            send_tag_command,
            send_layout_command,
            take_screenshot
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
