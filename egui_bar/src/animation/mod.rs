pub mod easing;

use egui::Color32;
use std::collections::HashMap;
use std::time::Instant;

use crate::config::{CONFIG, EasingName};
use easing::from_name;

#[derive(Debug, Clone)]
struct AnimEntry {
    start_value: f32,
    target_value: f32,
    start_time: Instant,
    duration_ms: u64,
    easing: fn(f32) -> f32,
}

/// Manages animation state for smooth transitions
#[derive(Debug)]
pub struct AnimationState {
    entries: HashMap<String, AnimEntry>,
}

impl AnimationState {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Animate a value toward a target. Returns the current interpolated value.
    pub fn animate(&mut self, id: &str, target: f32, duration_ms: u64, easing: EasingName) -> f32 {
        let cfg = CONFIG.load();
        if !cfg.animation.enabled {
            return target;
        }

        let now = Instant::now();

        let entry = self.entries.get(id);
        let needs_new = match entry {
            None => true,
            Some(e) => (e.target_value - target).abs() > f32::EPSILON,
        };

        if needs_new {
            let current = entry.map(|e| self.eval_entry(e, now)).unwrap_or(target);
            self.entries.insert(
                id.to_string(),
                AnimEntry {
                    start_value: current,
                    target_value: target,
                    start_time: now,
                    duration_ms,
                    easing: from_name(easing),
                },
            );
        }

        let entry = &self.entries[id];
        self.eval_entry(entry, now)
    }

    /// Animate a color toward a target color
    pub fn animate_color(
        &mut self,
        id: &str,
        target: Color32,
        duration_ms: u64,
        easing: EasingName,
    ) -> Color32 {
        let r = self.animate(&format!("{}_r", id), target.r() as f32, duration_ms, easing);
        let g = self.animate(&format!("{}_g", id), target.g() as f32, duration_ms, easing);
        let b = self.animate(&format!("{}_b", id), target.b() as f32, duration_ms, easing);
        let a = self.animate(&format!("{}_a", id), target.a() as f32, duration_ms, easing);
        Color32::from_rgba_unmultiplied(r as u8, g as u8, b as u8, a as u8)
    }

    /// Check if any animation is still in progress
    pub fn is_animating(&self) -> bool {
        let now = Instant::now();
        self.entries.values().any(|e| {
            let elapsed = now.duration_since(e.start_time).as_millis() as u64;
            elapsed < e.duration_ms
        })
    }

    fn eval_entry(&self, entry: &AnimEntry, now: Instant) -> f32 {
        let elapsed = now.duration_since(entry.start_time).as_millis() as u64;
        if elapsed >= entry.duration_ms {
            return entry.target_value;
        }

        let t = elapsed as f32 / entry.duration_ms as f32;
        let eased = (entry.easing)(t);
        entry.start_value + (entry.target_value - entry.start_value) * eased
    }

    /// Clean up completed animations to prevent unbounded growth
    pub fn gc(&mut self) {
        let now = Instant::now();
        self.entries.retain(|_, e| {
            let elapsed = now.duration_since(e.start_time).as_millis() as u64;
            elapsed < e.duration_ms + 1000 // keep 1s after completion for re-targeting
        });
    }
}
