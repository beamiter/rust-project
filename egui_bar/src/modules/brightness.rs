use std::fs;
use std::time::{Duration, Instant};

use crate::animation::AnimationState;
use crate::state::AppState;
use crate::theme::colors;

use super::BarModule;

pub struct BrightnessModule {
    backlight_path: Option<String>,
    brightness: u32,
    max_brightness: u32,
    last_poll: Instant,
    poll_interval: Duration,
}

impl BrightnessModule {
    pub fn new() -> Self {
        let backlight_path = Self::find_backlight();
        let mut module = Self {
            backlight_path,
            brightness: 0,
            max_brightness: 1,
            last_poll: Instant::now() - Duration::from_secs(10),
            poll_interval: Duration::from_secs(2),
        };
        module.poll();
        module
    }

    fn find_backlight() -> Option<String> {
        let entries = fs::read_dir("/sys/class/backlight").ok()?;
        for entry in entries.flatten() {
            return Some(entry.path().to_string_lossy().to_string());
        }
        None
    }

    fn poll(&mut self) {
        let Some(path) = &self.backlight_path else {
            return;
        };

        self.brightness = fs::read_to_string(format!("{}/brightness", path))
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);

        self.max_brightness = fs::read_to_string(format!("{}/max_brightness", path))
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(1);

        self.last_poll = Instant::now();
    }

    fn percent(&self) -> u32 {
        if self.max_brightness == 0 {
            return 0;
        }
        (self.brightness as f64 / self.max_brightness as f64 * 100.0).round() as u32
    }

    fn set_brightness(&self, value: u32) {
        let Some(_path) = &self.backlight_path else {
            return;
        };
        let clamped = value.min(self.max_brightness);
        // Writing to sysfs requires root; try brightnessctl as fallback
        let _ = std::process::Command::new("brightnessctl")
            .args(["set", &format!("{}", clamped)])
            .output();
    }

    fn brightness_icon(percent: u32) -> &'static str {
        match percent {
            0..=25 => "🔅",
            26..=75 => "🔆",
            _ => "☀️",
        }
    }
}

impl BarModule for BrightnessModule {
    fn id(&self) -> &str {
        "brightness"
    }

    fn name(&self) -> &str {
        "Brightness"
    }

    fn update(&mut self, _state: &AppState) -> bool {
        if self.last_poll.elapsed() >= self.poll_interval {
            self.poll();
            true
        } else {
            false
        }
    }

    fn render_bar(&mut self, ui: &mut egui::Ui, _state: &mut AppState, _anim: &mut AnimationState) {
        if self.backlight_path.is_none() {
            return; // No backlight device, hide module
        }

        let pct = self.percent();
        let icon = Self::brightness_icon(pct);

        let resp = ui.add(egui::Button::new(
            egui::RichText::new(format!("{} {}%", icon, pct)).color(colors::WHEAT),
        ));

        let hovered = resp.hovered();
        resp.on_hover_text(format!("Brightness: {}%\nScroll to adjust", pct));

        // Handle scroll to adjust brightness
        if hovered {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll.abs() > 0.5 {
                let step = (self.max_brightness as f64 * 0.05).max(1.0) as i64;
                let delta = if scroll > 0.0 { step } else { -step };
                let new_val = (self.brightness as i64 + delta)
                    .max(0)
                    .min(self.max_brightness as i64) as u32;
                self.set_brightness(new_val);
                self.brightness = new_val;
            }
        }
    }
}
