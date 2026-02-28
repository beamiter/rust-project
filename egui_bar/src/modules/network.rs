use std::fs;
use std::time::{Duration, Instant};

use crate::animation::AnimationState;
use crate::state::AppState;
use crate::theme::colors;

use super::BarModule;

#[derive(Debug, Clone)]
struct NetworkInfo {
    interface: String,
    connected: bool,
    ip: Option<String>,
    is_wifi: bool,
    signal_strength: Option<i32>, // dBm
}

pub struct NetworkModule {
    info: Vec<NetworkInfo>,
    last_poll: Instant,
    poll_interval: Duration,
    show_popup: bool,
}

impl NetworkModule {
    pub fn new() -> Self {
        let mut module = Self {
            info: Vec::new(),
            last_poll: Instant::now() - Duration::from_secs(10),
            poll_interval: Duration::from_secs(5),
            show_popup: false,
        };
        module.poll();
        module
    }

    fn poll(&mut self) {
        self.info.clear();
        let Ok(entries) = fs::read_dir("/sys/class/net") else {
            return;
        };

        for entry in entries.flatten() {
            let iface = entry.file_name().to_string_lossy().to_string();
            if iface == "lo" {
                continue;
            }

            let operstate_path = format!("/sys/class/net/{}/operstate", iface);
            let connected = fs::read_to_string(&operstate_path)
                .map(|s| s.trim() == "up")
                .unwrap_or(false);

            let is_wifi = fs::metadata(format!("/sys/class/net/{}/wireless", iface)).is_ok();

            let signal_strength = if is_wifi {
                // Read from /proc/net/wireless
                fs::read_to_string("/proc/net/wireless")
                    .ok()
                    .and_then(|content| {
                        content.lines().find_map(|line| {
                            if line.trim_start().starts_with(&iface) {
                                // Format: iface: status link level noise ...
                                let parts: Vec<&str> = line.split_whitespace().collect();
                                parts.get(3).and_then(|s| {
                                    s.trim_end_matches('.').parse::<i32>().ok()
                                })
                            } else {
                                None
                            }
                        })
                    })
            } else {
                None
            };

            let ip = Self::get_ip_address(&iface);

            self.info.push(NetworkInfo {
                interface: iface,
                connected,
                ip,
                is_wifi,
                signal_strength,
            });
        }

        self.last_poll = Instant::now();
    }

    fn get_ip_address(iface: &str) -> Option<String> {
        // Read IP from /proc/net/fib_trie or use a simple approach via command
        // For simplicity, try reading from ip command output cached approach
        let output = std::process::Command::new("ip")
            .args(["-4", "-o", "addr", "show", iface])
            .output()
            .ok()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Format: 2: eth0    inet 192.168.1.100/24 ...
        stdout.split_whitespace().find_map(|part| {
            if part.contains('/') && part.contains('.') {
                Some(part.split('/').next().unwrap_or(part).to_string())
            } else {
                None
            }
        })
    }

    fn primary_interface(&self) -> Option<&NetworkInfo> {
        // Prefer connected wifi, then connected wired
        self.info
            .iter()
            .filter(|i| i.connected)
            .min_by_key(|i| if i.is_wifi { 0 } else { 1 })
            .or_else(|| self.info.first())
    }

    fn wifi_icon(signal: Option<i32>) -> &'static str {
        match signal {
            Some(s) if s > -50 => "📶",
            Some(s) if s > -70 => "📶",
            Some(_) => "📡",
            None => "📡",
        }
    }
}

impl BarModule for NetworkModule {
    fn id(&self) -> &str {
        "network"
    }

    fn name(&self) -> &str {
        "Network"
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
        let (icon, color, tooltip) = if let Some(primary) = self.primary_interface() {
            if primary.connected {
                let icon = if primary.is_wifi {
                    Self::wifi_icon(primary.signal_strength)
                } else {
                    "🔗"
                };
                let ip_str = primary.ip.as_deref().unwrap_or("no IP");
                (
                    icon,
                    colors::SUCCESS,
                    format!("{}: {} ({})", primary.interface, ip_str,
                        if primary.is_wifi { "WiFi" } else { "Wired" }),
                )
            } else {
                ("❌", colors::ERROR, format!("{}: disconnected", primary.interface))
            }
        } else {
            ("❌", colors::TEXT_SUBTLE, "No network interface".to_string())
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
        egui::Window::new("🌐 Network")
            .collapsible(false)
            .resizable(false)
            .default_width(300.0)
            .open(&mut open)
            .show(ctx, |ui| {
                for info in &self.info {
                    ui.horizontal(|ui| {
                        let icon = if info.is_wifi { "📡" } else { "🔗" };
                        let status_color = if info.connected { colors::SUCCESS } else { colors::ERROR };
                        ui.label(egui::RichText::new(icon).color(status_color));
                        ui.label(egui::RichText::new(&info.interface).strong());
                        if info.connected {
                            if let Some(ip) = &info.ip {
                                ui.label(ip);
                            }
                            if let Some(signal) = info.signal_strength {
                                ui.label(egui::RichText::new(format!("{}dBm", signal)).color(colors::TEXT_SUBTLE));
                            }
                        } else {
                            ui.label(egui::RichText::new("down").color(colors::ERROR));
                        }
                    });
                }
                if self.info.is_empty() {
                    ui.label("No network interfaces found");
                }
            });

        if !open {
            self.show_popup = false;
        }
    }
}
