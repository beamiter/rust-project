//! egui_bar - A modern system status bar application
//!
//! This application provides a customizable system status bar with features including:
//! - Audio volume control
//! - System resource monitoring  
//! - Workspace/tag information display
//! - Configurable themes and layouts

use chrono::Local;
use egui_bar::{app::EguiBarApp, config::AppConfig, utils::AppError};
use flexi_logger::{Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming};
use log::{error, info, warn};
use shared_structures::{SharedMessage, SharedRingBuffer};
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{env, u128};

/// Application entry point
fn main() -> eframe::Result<()> {
    // 修改返回类型
    // Parse command line arguments
    let args: Vec<String> = env::args().collect();
    let shared_path = args.get(1).cloned().unwrap_or_default();

    // Initialize logging
    if let Err(e) = initialize_logging(&shared_path) {
        eprintln!("Failed to initialize logging: {}", e);
        std::process::exit(1);
    }

    info!("Starting egui_bar v{}", egui_bar::VERSION);

    // Load configuration
    let config = match AppConfig::load() {
        Ok(mut config) => {
            config.validate().unwrap_or_else(|e| {
                warn!("Configuration validation failed: {}", e);
            });
            config
        }
        Err(e) => {
            error!("Failed to load configuration: {}", e);
            AppConfig::default()
        }
    };

    // Create communication channels
    let (message_sender, message_receiver) = mpsc::channel();
    let (resize_sender, resize_receiver) = mpsc::channel();
    let (heartbeat_sender, heartbeat_receiver) = mpsc::channel();

    // Start background thread for shared memory monitoring
    let shared_path_clone = shared_path.clone();
    thread::spawn(move || {
        background_worker(
            shared_path_clone,
            message_sender,
            resize_receiver,
            heartbeat_sender,
        )
    });

    // Start heartbeat monitor
    thread::spawn(move || heartbeat_monitor(heartbeat_receiver));

    // Configure eframe options
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_position(egui::Pos2::new(0.0, 0.0))
            .with_inner_size([800.0, config.ui.font_size * 2.0])
            .with_min_inner_size([480.0, config.ui.font_size])
            .with_decorations(false)
            .with_transparent(config.ui.window_opacity < 1.0),
        vsync: true,
        ..Default::default()
    };

    // Run the application
    eframe::run_native(
        "egui_bar",
        native_options,
        Box::new(move |cc| {
            // 修复：直接处理错误而不是使用 Custom 变体
            match EguiBarApp::new(cc, message_receiver, resize_sender) {
                Ok(app) => {
                    info!("Application created successfully");
                    Ok(Box::new(app))
                }
                Err(e) => {
                    error!("Failed to create application: {}", e);
                    // 直接退出程序而不是返回自定义错误
                    std::process::exit(1);
                }
            }
        }),
    )
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
        .duplicate_to_stdout(Duplicate::Info)
        .rotate(
            Criterion::Size(10_000_000), // 10MB
            Naming::Numbers,
            Cleanup::KeepLogFiles(5),
        )
        .start()
        .map_err(|e| AppError::config(format!("Failed to start logger: {}", e)))?;

    Ok(())
}

/// Background worker thread for shared memory monitoring
fn background_worker(
    shared_path: String,
    message_sender: mpsc::Sender<SharedMessage>,
    resize_receiver: mpsc::Receiver<bool>,
    heartbeat_sender: mpsc::Sender<()>,
) {
    info!("Starting background worker thread");

    // Initialize shared ring buffer
    let ring_buffer: Option<SharedRingBuffer> = if shared_path.is_empty() {
        warn!("No shared path provided, running without shared memory");
        None
    } else {
        match SharedRingBuffer::open(&shared_path) {
            Ok(rb) => {
                info!("Successfully opened shared ring buffer: {}", shared_path);
                Some(rb)
            }
            Err(e) => {
                error!("Failed to open shared ring buffer '{}': {}", shared_path, e);
                None
            }
        }
    };

    // Set panic hook
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        error!("Background thread panicked: {}", panic_info);
        default_hook(panic_info);
    }));

    let mut prev_timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();

    let mut frame_count: u128 = 0;
    let mut error_count = 0;
    const MAX_ERROR_LOGS: usize = 10;

    loop {
        // Handle ring buffer messages
        if let Some(ref rb) = ring_buffer {
            match rb.try_read_latest_message::<SharedMessage>() {
                Ok(Some(message)) => {
                    if prev_timestamp != message.timestamp {
                        prev_timestamp = message.timestamp;

                        if let Err(e) = message_sender.send(message) {
                            error!("Failed to send message to UI thread: {}", e);
                            break; // UI thread probably died
                        }

                        error_count = 0; // Reset error count on success
                    }
                }
                Ok(None) => {
                    // No new messages - this is normal
                    if frame_count % 1000 == 0 {
                        info!("No new messages (frame {})", frame_count);
                    }
                }
                Err(e) => {
                    if error_count < MAX_ERROR_LOGS {
                        error!("Ring buffer read error: {}", e);
                        error_count += 1;
                    } else if error_count == MAX_ERROR_LOGS {
                        error!("Ring buffer error limit reached, suppressing further errors");
                        error_count += 1;
                    }
                }
            }
        }

        // Handle resize requests
        while resize_receiver.try_recv().is_ok() {
            // Just consume resize requests - the actual resizing is handled in the UI thread
        }

        // Send heartbeat
        if heartbeat_sender.send(()).is_err() {
            warn!("Heartbeat receiver disconnected, exiting background thread");
            break;
        }

        frame_count = frame_count.wrapping_add(1);

        // Sleep to control update rate (100 FPS)
        thread::sleep(Duration::from_millis(10));
    }

    info!("Background worker thread exiting");
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
                error!("Background thread heartbeat timeout - thread may have died");
                std::process::exit(1);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                error!("Background thread heartbeat disconnected - thread died");
                std::process::exit(1);
            }
        }
    }
}
