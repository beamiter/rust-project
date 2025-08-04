//! egui_bar - A modern system status bar application

use chrono::Local;
use egui_bar::{AppError, EguiBarApp};
use flexi_logger::{Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming};
use log::{error, info, warn};
use shared_structures::{SharedCommand, SharedMessage, SharedRingBuffer};
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{env, u128};

/// Application entry point
fn main() -> eframe::Result<()> {
    // Parse command line arguments
    let args: Vec<String> = env::args().collect();
    let shared_path = args.get(1).cloned().unwrap_or_default();

    // Initialize logging
    if let Err(e) = initialize_logging(&shared_path) {
        eprintln!("Failed to initialize logging: {}", e);
        std::process::exit(1);
    }

    info!("Starting egui_bar V1.0");

    // Create communication channels
    let (message_sender, message_receiver) = mpsc::channel::<SharedMessage>();
    let (command_sender, command_receiver) = mpsc::channel::<SharedCommand>();
    let (heartbeat_sender, heartbeat_receiver) = mpsc::channel();

    let shared_path_clone = shared_path.clone();
    thread::spawn(move || {
        shared_memory_worker(
            shared_path_clone,
            message_sender,
            heartbeat_sender,
            command_receiver,
        )
    });

    // Start heartbeat monitor
    thread::spawn(move || heartbeat_monitor(heartbeat_receiver));

    // Configure eframe options
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_position(egui::Pos2::new(0.0, 0.0))
            .with_inner_size([1080.0, 40.])
            .with_min_inner_size([480.0, 20.])
            .with_decorations(false)
            .with_resizable(true)
            .with_transparent(false),
        vsync: true,
        ..Default::default()
    };

    // Run the application
    eframe::run_native(
        "egui_bar",
        native_options,
        Box::new(
            move |cc| match EguiBarApp::new(cc, message_receiver, command_sender) {
                Ok(app) => {
                    // don't do this every frame - only when the app is created!
                    egui_extras::install_image_loaders(&cc.egui_ctx);
                    info!("Application created successfully");
                    Ok(Box::new(app))
                }
                Err(e) => {
                    error!("Failed to create application: {}", e);
                    std::process::exit(1);
                }
            },
        ),
    )
}

fn shared_memory_worker(
    shared_path: String,
    message_sender: mpsc::Sender<SharedMessage>,
    heartbeat_sender: mpsc::Sender<()>,
    command_receiver: mpsc::Receiver<SharedCommand>,
) {
    info!("Starting shared memory worker thread");

    // 尝试打开或创建共享环形缓冲区
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

    let mut prev_timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();

    let mut frame_count: u128 = 0;
    let mut consecutive_errors = 0;

    loop {
        // 发送心跳信号
        if heartbeat_sender.send(()).is_err() {
            warn!("Heartbeat receiver disconnected");
            break;
        }

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
            match shared_buffer.try_read_latest_message() {
                Ok(Some(message)) => {
                    consecutive_errors = 0; // 成功读取，重置错误计数
                    if prev_timestamp != message.timestamp.into() {
                        prev_timestamp = message.timestamp.into();
                        if let Err(e) = message_sender.send(message) {
                            error!("Failed to send message: {}", e);
                            break;
                        }
                    }
                }
                Ok(None) => {
                    // 没有新消息，这是正常的
                    consecutive_errors = 0;
                }
                Err(e) => {
                    consecutive_errors += 1;
                    if frame_count % 1000 == 0 || consecutive_errors == 1 {
                        error!(
                            "Ring buffer read error: {}. Buffer state: available={}",
                            e,
                            shared_buffer.available_messages(),
                        );
                    }

                    // 如果连续错误过多，尝试重置读取位置
                    if consecutive_errors > 10 {
                        warn!("Too many consecutive errors, resetting read index");
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

/// Initialize logging system
fn initialize_logging(shared_path: &str) -> Result<(), AppError> {
    let now = Local::now();
    let timestamp = now.format("%Y-%m-%d_%H_%M_%S").to_string();

    let file_name = if shared_path.is_empty() {
        "egui_bar".to_string()
    } else {
        Path::new(shared_path)
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| format!("egui_bar_{}", name))
            .unwrap_or_else(|| "egui_bar".to_string())
    };

    let log_filename = format!("{}_{}", file_name, timestamp);

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

/// Monitor heartbeat from background thread
fn heartbeat_monitor(heartbeat_receiver: mpsc::Receiver<()>) {
    info!("Starting heartbeat monitor");

    loop {
        match heartbeat_receiver.recv_timeout(Duration::from_secs(5)) {
            Ok(_) => {
                // Heartbeat received, continue
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                error!("Shared memory thread heartbeat timeout");
                std::process::exit(1);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                error!("Shared memory thread disconnected");
                std::process::exit(1);
            }
        }
    }
}
