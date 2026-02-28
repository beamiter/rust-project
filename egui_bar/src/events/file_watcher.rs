use log::{error, info, warn};
use notify::{EventKind, RecursiveMode, Watcher};
use std::sync::mpsc;

use super::BarEvent;

/// Watch backlight brightness sysfs files for changes
pub fn start_brightness_watcher(
    event_tx: mpsc::Sender<BarEvent>,
    egui_ctx: egui::Context,
) {
    let backlight_dir = std::path::PathBuf::from("/sys/class/backlight");
    if !backlight_dir.exists() {
        info!("No backlight sysfs, skipping brightness watcher");
        return;
    }

    // Find the first backlight device
    let backlight_path = match std::fs::read_dir(&backlight_dir) {
        Ok(mut entries) => entries.next().and_then(|e| e.ok()).map(|e| e.path()),
        Err(_) => None,
    };

    let Some(device_path) = backlight_path else {
        info!("No backlight device found, skipping brightness watcher");
        return;
    };

    let brightness_file = device_path.join("brightness");
    let max_brightness: u32 = std::fs::read_to_string(device_path.join("max_brightness"))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(1);

    std::thread::spawn(move || {
        let (tx, rx) = std::sync::mpsc::channel();

        let mut watcher = match notify::recommended_watcher(
            move |res: notify::Result<notify::Event>| {
                if let Ok(event) = res {
                    let _ = tx.send(event);
                }
            },
        ) {
            Ok(w) => w,
            Err(e) => {
                error!("Failed to create brightness watcher: {}", e);
                return;
            }
        };

        if let Err(e) = watcher.watch(&device_path, RecursiveMode::NonRecursive) {
            warn!("Failed to watch backlight sysfs {:?}: {}", device_path, e);
            return;
        }

        info!("Brightness file watcher started for {:?}", device_path);

        for event in rx {
            let dominated = matches!(event.kind, EventKind::Modify(_));

            if dominated && event.paths.iter().any(|p| p == &brightness_file) {
                if let Some(value) = std::fs::read_to_string(&brightness_file)
                    .ok()
                    .and_then(|s| s.trim().parse::<u32>().ok())
                {
                    let _ = event_tx.send(BarEvent::BrightnessChanged {
                        value,
                        max: max_brightness,
                    });
                    egui_ctx.request_repaint();
                }
            }
        }
    });
}
