//! egui_bar - A modern system status bar application

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
    let (resize_sender, _resize_receiver) = mpsc::channel(); // 不再需要 resize_receiver
    let (heartbeat_sender, heartbeat_receiver) = mpsc::channel();

    // Start shared memory monitoring thread (简化版本)
    let shared_path_clone = shared_path.clone();
    thread::spawn(move || {
        shared_memory_worker(shared_path_clone, message_sender, heartbeat_sender)
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
        Box::new(
            move |cc| match EguiBarApp::new(cc, message_receiver, resize_sender) {
                Ok(app) => {
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

/// 简化的共享内存工作线程
fn shared_memory_worker(
    shared_path: String,
    message_sender: mpsc::Sender<SharedMessage>,
    heartbeat_sender: mpsc::Sender<()>,
) {
    info!("Starting shared memory worker thread");

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

    let mut prev_timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();

    let mut frame_count: u128 = 0;

    loop {
        // Handle ring buffer messages
        if let Some(ref rb) = ring_buffer {
            match rb.try_read_latest_message::<SharedMessage>() {
                Ok(Some(message)) => {
                    if prev_timestamp != message.timestamp {
                        prev_timestamp = message.timestamp;

                        if let Err(e) = message_sender.send(message) {
                            error!("Failed to send message: {}", e);
                            break;
                        }
                    }
                }
                Ok(None) => {
                    // No new messages - normal
                }
                Err(e) => {
                    if frame_count % 1000 == 0 {
                        error!("Ring buffer read error: {}", e);
                    }
                }
            }
        }

        // Send heartbeat
        if heartbeat_sender.send(()).is_err() {
            warn!("Heartbeat receiver disconnected");
            break;
        }

        frame_count = frame_count.wrapping_add(1);
        thread::sleep(Duration::from_millis(50)); // 20 FPS for shared memory polling
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
