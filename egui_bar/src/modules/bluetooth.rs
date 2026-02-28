use std::time::{Duration, Instant};

use crate::animation::AnimationState;
use crate::state::AppState;
use crate::theme::colors;

use super::BarModule;

#[derive(Debug, Clone)]
struct BluetoothDevice {
    name: String,
    mac: String,
    connected: bool,
}

#[derive(Debug, Clone)]
struct BluetoothState {
    powered: bool,
    devices: Vec<BluetoothDevice>,
}

pub struct BluetoothModule {
    bt_state: BluetoothState,
    last_poll: Instant,
    poll_interval: Duration,
    show_popup: bool,
}

impl BluetoothModule {
    pub fn new() -> Self {
        let mut module = Self {
            bt_state: BluetoothState {
                powered: false,
                devices: Vec::new(),
            },
            last_poll: Instant::now() - Duration::from_secs(10),
            poll_interval: Duration::from_secs(10),
            show_popup: false,
        };
        module.poll();
        module
    }

    fn poll(&mut self) {
        // Check if bluetooth controller exists
        let has_bt = std::fs::read_dir("/sys/class/bluetooth").is_ok();
        if !has_bt {
            self.bt_state.powered = false;
            self.bt_state.devices.clear();
            self.last_poll = Instant::now();
            return;
        }

        // Use bluetoothctl to get device info
        self.bt_state.powered = Self::is_powered();
        if self.bt_state.powered {
            self.bt_state.devices = Self::get_connected_devices();
        } else {
            self.bt_state.devices.clear();
        }

        self.last_poll = Instant::now();
    }

    fn is_powered() -> bool {
        std::process::Command::new("bluetoothctl")
            .args(["show"])
            .output()
            .ok()
            .map(|o| {
                let s = String::from_utf8_lossy(&o.stdout);
                s.contains("Powered: yes")
            })
            .unwrap_or(false)
    }

    fn get_connected_devices() -> Vec<BluetoothDevice> {
        let output = match std::process::Command::new("bluetoothctl")
            .args(["devices", "Connected"])
            .output()
        {
            Ok(o) => o,
            Err(_) => return Vec::new(),
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout
            .lines()
            .filter_map(|line| {
                // Format: Device XX:XX:XX:XX:XX:XX DeviceName
                let parts: Vec<&str> = line.splitn(3, ' ').collect();
                if parts.len() == 3 && parts[0] == "Device" {
                    Some(BluetoothDevice {
                        mac: parts[1].to_string(),
                        name: parts[2].to_string(),
                        connected: true,
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    fn connected_count(&self) -> usize {
        self.bt_state.devices.iter().filter(|d| d.connected).count()
    }
}

impl BarModule for BluetoothModule {
    fn id(&self) -> &str {
        "bluetooth"
    }

    fn name(&self) -> &str {
        "Bluetooth"
    }

    fn update(&mut self, _state: &AppState) -> bool {
        if self.last_poll.elapsed() >= self.poll_interval {
            self.poll();
            true
        } else {
            false
        }
    }

    fn has_popup(&self) -> bool {
        true
    }

    fn render_bar(&mut self, ui: &mut egui::Ui, _state: &mut AppState, _anim: &mut AnimationState) {
        let (icon, color, tooltip) = if !self.bt_state.powered {
            ("", colors::TEXT_SUBTLE, "Bluetooth off".to_string())
        } else {
            let count = self.connected_count();
            if count > 0 {
                (
                    "",
                    colors::ACCENT_PRIMARY,
                    format!("Bluetooth: {} device(s) connected", count),
                )
            } else {
                ("", colors::TEXT_SUBTLE, "Bluetooth on, no devices".to_string())
            }
        };

        let resp = ui.add(egui::Button::new(egui::RichText::new(icon).color(color)));
        if resp.clicked() {
            self.show_popup = !self.show_popup;
        }
        resp.on_hover_text(tooltip);
    }

    fn render_popup(&mut self, ctx: &egui::Context, _state: &mut AppState) {
        if !self.show_popup {
            return;
        }

        let mut open = true;
        egui::Window::new(" Bluetooth")
            .collapsible(false)
            .resizable(false)
            .default_width(280.0)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Power:");
                    if self.bt_state.powered {
                        ui.label(egui::RichText::new("ON").color(colors::SUCCESS));
                    } else {
                        ui.label(egui::RichText::new("OFF").color(colors::ERROR));
                    }
                });

                ui.separator();

                if self.bt_state.devices.is_empty() {
                    ui.label(egui::RichText::new("No connected devices").color(colors::TEXT_SUBTLE));
                } else {
                    for device in &self.bt_state.devices {
                        ui.horizontal(|ui| {
                            let status = if device.connected { "🟢" } else { "⚪" };
                            ui.label(status);
                            ui.label(&device.name);
                            ui.label(egui::RichText::new(&device.mac).color(colors::TEXT_SUBTLE).small());
                        });
                    }
                }

                ui.separator();
                if ui.small_button("🔄 Refresh").clicked() {
                    self.poll();
                }
            });

        if !open {
            self.show_popup = false;
        }
    }
}
