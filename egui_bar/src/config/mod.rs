pub mod schema;

pub use schema::*;

use arc_swap::ArcSwap;
use log::{error, info, warn};
use once_cell::sync::Lazy;
use std::path::PathBuf;
use std::sync::Arc;

/// Global configuration, lock-free reads via ArcSwap
pub static CONFIG: Lazy<ArcSwap<BarConfig>> = Lazy::new(|| {
    let config = load_config_from_disk().unwrap_or_default();
    ArcSwap::from_pointee(config)
});

/// Get the default config file path: ~/.config/egui_bar/config.toml
pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("egui_bar")
        .join("config.toml")
}

/// Load config from the default path, falling back to defaults
fn load_config_from_disk() -> Option<BarConfig> {
    let path = config_path();
    if !path.exists() {
        info!("No config file at {:?}, using defaults", path);
        return None;
    }

    match std::fs::read_to_string(&path) {
        Ok(contents) => match toml::from_str::<BarConfig>(&contents) {
            Ok(config) => {
                info!("Loaded config from {:?}", path);
                Some(config)
            }
            Err(e) => {
                error!("Failed to parse config {:?}: {}", path, e);
                None
            }
        },
        Err(e) => {
            error!("Failed to read config {:?}: {}", path, e);
            None
        }
    }
}

/// Reload the global CONFIG from disk
pub fn reload_global() -> anyhow::Result<()> {
    let config = load_config_from_disk().unwrap_or_default();
    CONFIG.store(Arc::new(config));
    info!("Config reloaded successfully");
    Ok(())
}

/// Start a file watcher for config hot-reload
pub fn start_config_watcher(egui_ctx: egui::Context) {
    use notify::{EventKind, RecursiveMode, Watcher};

    let path = config_path();
    let watch_dir = match path.parent() {
        Some(dir) => dir.to_path_buf(),
        None => {
            warn!("Cannot determine config directory for watching");
            return;
        }
    };

    // Ensure the config directory exists
    if !watch_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&watch_dir) {
            warn!("Failed to create config directory {:?}: {}", watch_dir, e);
            return;
        }
    }

    let config_file = path.clone();
    std::thread::spawn(move || {
        let (tx, rx) = std::sync::mpsc::channel();

        let mut watcher = match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                let _ = tx.send(event);
            }
        }) {
            Ok(w) => w,
            Err(e) => {
                error!("Failed to create config file watcher: {}", e);
                return;
            }
        };

        if let Err(e) = watcher.watch(&watch_dir, RecursiveMode::NonRecursive) {
            error!("Failed to watch config directory {:?}: {}", watch_dir, e);
            return;
        }

        info!("Config file watcher started for {:?}", watch_dir);

        for event in rx {
            let dominated = matches!(
                event.kind,
                EventKind::Modify(_) | EventKind::Create(_)
            );

            if dominated && event.paths.iter().any(|p| p == &config_file) {
                info!("Config file changed, reloading...");
                // Small delay to let writes settle
                std::thread::sleep(std::time::Duration::from_millis(100));
                if let Err(e) = reload_global() {
                    error!("Config reload failed: {}", e);
                }
                egui_ctx.request_repaint();
            }
        }
    });
}
