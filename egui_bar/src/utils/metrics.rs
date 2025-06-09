//! Performance metrics and monitoring

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Rolling average calculator
#[derive(Debug, Clone)]
pub struct RollingAverage {
    values: VecDeque<f64>,
    capacity: usize,
    sum: f64,
}

impl RollingAverage {
    pub fn new(capacity: usize) -> Self {
        Self {
            values: VecDeque::with_capacity(capacity),
            capacity,
            sum: 0.0,
        }
    }

    pub fn add(&mut self, value: f64) {
        if self.values.len() >= self.capacity {
            if let Some(old_value) = self.values.pop_front() {
                self.sum -= old_value;
            }
        }

        self.values.push_back(value);
        self.sum += value;
    }

    pub fn average(&self) -> f64 {
        if self.values.is_empty() {
            0.0
        } else {
            self.sum / self.values.len() as f64
        }
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// Performance metrics collector
#[derive(Debug)]
pub struct PerformanceMetrics {
    frame_times: RollingAverage,
    render_times: RollingAverage,
    update_times: RollingAverage,
    last_frame_start: Instant,
    frame_count: u64,
}

impl PerformanceMetrics {
    pub fn new() -> Self {
        Self {
            frame_times: RollingAverage::new(60),
            render_times: RollingAverage::new(60),
            update_times: RollingAverage::new(60),
            last_frame_start: Instant::now(),
            frame_count: 0,
        }
    }

    pub fn start_frame(&mut self) {
        let now = Instant::now();
        if self.frame_count > 0 {
            let frame_time = now.duration_since(self.last_frame_start);
            self.frame_times.add(frame_time.as_secs_f64());
        }
        self.last_frame_start = now;
        self.frame_count += 1;
    }

    pub fn record_render_time(&mut self, duration: Duration) {
        self.render_times.add(duration.as_secs_f64());
    }

    pub fn record_update_time(&mut self, duration: Duration) {
        self.update_times.add(duration.as_secs_f64());
    }

    pub fn average_fps(&self) -> f64 {
        let avg_frame_time = self.frame_times.average();
        if avg_frame_time > 0.0 {
            1.0 / avg_frame_time
        } else {
            0.0
        }
    }

    pub fn average_frame_time_ms(&self) -> f64 {
        self.frame_times.average() * 1000.0
    }

    pub fn average_render_time_ms(&self) -> f64 {
        self.render_times.average() * 1000.0
    }

    pub fn average_update_time_ms(&self) -> f64 {
        self.update_times.average() * 1000.0
    }

    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }
}

impl Default for PerformanceMetrics {
    fn default() -> Self {
        Self::new()
    }
}
