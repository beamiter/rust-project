use log::{debug, error, info, warn};
use shared_structures::{CommandType, SharedCommand, SharedMessage, SharedRingBuffer};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::state::SharedAppState;

/// Start background worker tasks (no tokio)
pub fn start_background_tasks(
    shared_state: &Arc<Mutex<SharedAppState>>,
    egui_ctx: &egui::Context,
    shared_buffer_rc: Option<Arc<SharedRingBuffer>>,
) {
    // Shared memory worker
    {
        let shared_state_clone = Arc::clone(shared_state);
        let egui_ctx_clone = egui_ctx.clone();
        if let Some(shared_buffer) = shared_buffer_rc {
            thread::spawn(move || {
                shared_memory_worker(shared_buffer, shared_state_clone, egui_ctx_clone);
            });
        }
    }

    // Periodic update task
    {
        let egui_ctx_clone = egui_ctx.clone();
        thread::spawn(move || {
            periodic_update_task(egui_ctx_clone);
        });
    }
}

/// Shared memory worker task (blocking loop)
pub fn shared_memory_worker(
    shared_buffer: Arc<SharedRingBuffer>,
    shared_state: Arc<Mutex<SharedAppState>>,
    egui_ctx: egui::Context,
) {
    info!("Starting shared memory worker task");
    let mut prev_timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    loop {
        match shared_buffer.wait_for_message(Some(Duration::from_secs(2))) {
            Ok(true) => {
                if let Ok(Some(message)) = shared_buffer.try_read_latest_message() {
                    if prev_timestamp != message.timestamp.into() {
                        prev_timestamp = message.timestamp.into();
                        if let Ok(mut state) = shared_state.lock() {
                            let need_update = state
                                .current_message
                                .as_ref()
                                .map(|m| m.timestamp != message.timestamp)
                                .unwrap_or(true);
                            if need_update {
                                info!("current_message: {:?}", message);
                                state.current_message = Some(message);
                                state.last_update = Instant::now();
                                egui_ctx.request_repaint();
                            }
                        } else {
                            warn!("Failed to lock shared state for message update");
                        }
                    }
                }
            }
            Ok(false) => {
                debug!("[notifier] Wait for message timed out.");
            }
            Err(e) => {
                error!("[notifier] Wait for message failed: {}", e);
                break;
            }
        }
    }

    info!("Shared memory worker task exiting");
}

/// Periodic update task for time display (no tokio)
pub fn periodic_update_task(egui_ctx: egui::Context) {
    info!("Starting periodic update task");
    let mut last_second = chrono::Local::now().timestamp();

    loop {
        thread::sleep(Duration::from_millis(500));
        let current_second = chrono::Local::now().timestamp();
        if current_second != last_second {
            last_second = current_second;
            egui_ctx.request_repaint();
        }
    }
}

/// Send a generic command via shared buffer
pub fn send_command(
    shared_buffer: &Option<Arc<SharedRingBuffer>>,
    command: SharedCommand,
) {
    if let Some(shared_buffer) = shared_buffer {
        match shared_buffer.send_command(command) {
            Ok(true) => info!("Sent command: {:?} by shared_buffer", command),
            Ok(false) => warn!("Command buffer full, command dropped"),
            Err(e) => error!("Failed to send command: {}", e),
        }
    }
}

/// Send tag-related commands
pub fn send_tag_command(
    shared_buffer: &Option<Arc<SharedRingBuffer>>,
    current_message: &Option<SharedMessage>,
    tag_bit: u32,
    is_view: bool,
) {
    if let Some(message) = current_message {
        let monitor_id = message.monitor_info.monitor_num;

        let command = if is_view {
            SharedCommand::view_tag(tag_bit, monitor_id)
        } else {
            SharedCommand::toggle_tag(tag_bit, monitor_id)
        };

        send_command(shared_buffer, command);
    }
}

/// Send layout change command
pub fn send_layout_command(
    shared_buffer: &Option<Arc<SharedRingBuffer>>,
    current_message: &Option<SharedMessage>,
    layout_index: u32,
) {
    if let Some(message) = current_message {
        let monitor_id = message.monitor_info.monitor_num;
        let command = SharedCommand::new(CommandType::SetLayout, layout_index, monitor_id);
        send_command(shared_buffer, command);
    }
}
