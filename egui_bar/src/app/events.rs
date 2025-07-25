//! Event handling system for the application
use crate::audio::AudioDevice;
use crate::system::SystemSnapshot;
use log::info;
use std::sync::mpsc;

/// Application events
#[derive(Debug, Clone)]
pub enum AppEvent {
    /// System information updated
    SystemUpdated(SystemSnapshot),

    /// Audio device state changed
    AudioDeviceChanged {
        device_name: String,
        volume: i32,
        is_muted: bool,
    },

    /// Volume adjustment requested
    VolumeAdjust { device_name: String, delta: i32 },

    /// Mute toggle requested
    ToggleMute(String),

    /// Audio device list refreshed
    AudioDevicesRefreshed(Vec<AudioDevice>),

    /// Time format toggle
    TimeFormatToggle,

    /// Screenshot requested
    ScreenshotRequested,

    /// Settings window toggle
    SettingsToggle,

    /// Debug window toggle
    DebugToggle,

    /// Application shutdown requested
    Shutdown,
}

/// Event bus for handling application events
pub struct EventBus {
    sender: mpsc::Sender<AppEvent>,
    receiver: mpsc::Receiver<AppEvent>,
}

impl EventBus {
    /// Create new event bus
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self { sender, receiver }
    }

    /// Get event sender
    pub fn sender(&self) -> mpsc::Sender<AppEvent> {
        self.sender.clone()
    }

    /// Process all pending events
    pub fn process_events<F>(&self, mut handler: F)
    where
        F: FnMut(AppEvent),
    {
        while let Ok(event) = self.receiver.try_recv() {
            info!("[process_events]");
            handler(event);
        }
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}
