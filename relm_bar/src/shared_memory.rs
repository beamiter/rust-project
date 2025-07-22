use glib;
use log::{error, info, warn};
use shared_structures::{SharedCommand, SharedMessage, SharedRingBuffer};
use std::{sync::mpsc::Sender, time::{Duration, SystemTime, UNIX_EPOCH}};

pub struct SharedMemoryWorker;

impl SharedMemoryWorker {
    pub fn start(shared_path: String, sender: Sender<SharedMessage>) {
        glib::spawn_future(async move {
            Self::run_worker(shared_path, sender).await;
        });
    }

    async fn run_worker(shared_path: String, sender: Sender<SharedMessage>) {
        info!("Starting shared memory worker");

        let shared_buffer_opt = if shared_path.is_empty() {
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
                        "Failed to open shared ring buffer: {}, attempting to create",
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

        let mut consecutive_errors = 0;

        loop {
            if let Some(ref shared_buffer) = shared_buffer_opt {
                match shared_buffer.try_read_latest_message::<SharedMessage>() {
                    Ok(Some(message)) => {
                        consecutive_errors = 0;
                        if prev_timestamp != message.timestamp {
                            prev_timestamp = message.timestamp;
                            if sender.send(message).is_err() {
                                error!("Failed to send message to main thread");
                                break;
                            }
                        }
                    }
                    Ok(None) => {
                        consecutive_errors = 0;
                    }
                    Err(e) => {
                        consecutive_errors += 1;
                        if consecutive_errors == 1 || consecutive_errors % 50 == 0 {
                            error!("Ring buffer read error ({}): {}", consecutive_errors, e);
                        }

                        if consecutive_errors > 50 {
                            warn!("Too many consecutive errors, resetting read index");
                            shared_buffer.reset_read_index();
                            consecutive_errors = 0;
                        }
                    }
                }
            }

            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        info!("Shared memory worker exiting");
    }
}
