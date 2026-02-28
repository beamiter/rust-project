use log::info;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use super::BarEvent;

/// Start DBus signal listeners in background threads.
///
/// Currently monitors:
/// - UPower battery status changes (via polling /sys as DBus requires async runtime)
/// - Network interface changes (via polling /sys/class/net)
///
/// These run as lightweight polling threads that only send events on actual changes,
/// significantly reducing UI update overhead compared to the previous approach
/// of polling everything in the main update() loop.
pub fn start_dbus_listeners(
    event_tx: mpsc::Sender<BarEvent>,
    egui_ctx: egui::Context,
) {
    // Battery monitor thread
    {
        let tx = event_tx.clone();
        let ctx = egui_ctx.clone();
        thread::spawn(move || {
            listen_battery(tx, ctx);
        });
    }

    // Network monitor thread
    {
        let tx = event_tx.clone();
        let ctx = egui_ctx.clone();
        thread::spawn(move || {
            listen_network(tx, ctx);
        });
    }
}

/// Monitor battery changes via sysfs polling
fn listen_battery(tx: mpsc::Sender<BarEvent>, ctx: egui::Context) {
    info!("Battery event listener started");

    let mut last_percent: Option<f32> = None;
    let mut last_charging: Option<bool> = None;

    loop {
        thread::sleep(Duration::from_secs(30));

        let percent = read_sysfs_f32("/sys/class/power_supply/BAT0/capacity");
        let charging = std::fs::read_to_string("/sys/class/power_supply/BAT0/status")
            .map(|s| s.trim() != "Discharging")
            .ok();

        if let (Some(pct), Some(chg)) = (percent, charging) {
            let changed = last_percent.map(|p| (p - pct).abs() > 0.5).unwrap_or(true)
                || last_charging.map(|c| c != chg).unwrap_or(true);

            if changed {
                last_percent = Some(pct);
                last_charging = Some(chg);

                let _ = tx.send(BarEvent::BatteryChanged {
                    percent: pct,
                    charging: chg,
                });
                ctx.request_repaint();
            }
        }
    }
}

/// Monitor network interface state changes via sysfs polling
fn listen_network(tx: mpsc::Sender<BarEvent>, ctx: egui::Context) {
    info!("Network event listener started");

    let mut last_states: std::collections::HashMap<String, bool> =
        std::collections::HashMap::new();

    loop {
        thread::sleep(Duration::from_secs(5));

        let Ok(entries) = std::fs::read_dir("/sys/class/net") else {
            continue;
        };

        for entry in entries.flatten() {
            let iface = entry.file_name().to_string_lossy().to_string();
            if iface == "lo" {
                continue;
            }

            let connected = std::fs::read_to_string(format!(
                "/sys/class/net/{}/operstate",
                iface
            ))
            .map(|s| s.trim() == "up")
            .unwrap_or(false);

            let changed = last_states
                .get(&iface)
                .map(|&prev| prev != connected)
                .unwrap_or(true);

            if changed {
                last_states.insert(iface.clone(), connected);
                let _ = tx.send(BarEvent::NetworkChanged {
                    interface: iface,
                    connected,
                });
                ctx.request_repaint();
            }
        }
    }
}

fn read_sysfs_f32(path: &str) -> Option<f32> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
}
