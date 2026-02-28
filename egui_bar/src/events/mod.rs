pub mod dbus_listener;
pub mod file_watcher;

use shared_structures::SharedMessage;
use std::sync::mpsc;

/// Events that flow through the event bus
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum BarEvent {
    /// Battery state changed
    BatteryChanged {
        percent: f32,
        charging: bool,
    },
    /// Network interface state changed
    NetworkChanged {
        interface: String,
        connected: bool,
    },
    /// Bluetooth device state changed
    BluetoothDeviceChanged {
        address: String,
        connected: bool,
    },
    /// Backlight brightness changed
    BrightnessChanged {
        value: u32,
        max: u32,
    },
    /// Media player state changed
    MediaPlayerChanged {
        player: String,
        title: String,
        artist: String,
    },
    /// Media playback state changed
    MediaPlaybackChanged {
        status: String,
    },
    /// Workspace/tag changed via shared memory
    WorkspaceChanged {
        message: SharedMessage,
    },
    /// Configuration file was reloaded
    ConfigReloaded,
    /// System tray item added
    TrayItemAdded {
        id: String,
    },
    /// System tray item removed
    TrayItemRemoved {
        id: String,
    },
}

/// Event bus for distributing events to modules
pub struct EventBus {
    tx: mpsc::Sender<BarEvent>,
    rx: mpsc::Receiver<BarEvent>,
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self { tx, rx }
    }

    /// Get a sender handle (cloneable, send to background threads)
    pub fn sender(&self) -> mpsc::Sender<BarEvent> {
        self.tx.clone()
    }

    /// Drain all pending events (non-blocking)
    pub fn drain(&self) -> Vec<BarEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.rx.try_recv() {
            events.push(event);
        }
        events
    }
}

/// Start all event listeners
pub fn start_event_listeners(
    event_tx: mpsc::Sender<BarEvent>,
    egui_ctx: egui::Context,
) {
    // DBus listeners for battery, network changes
    dbus_listener::start_dbus_listeners(event_tx.clone(), egui_ctx.clone());

    // File watcher for brightness sysfs
    file_watcher::start_brightness_watcher(event_tx, egui_ctx);
}
