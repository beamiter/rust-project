//! System monitoring with caching and efficient updates

use crate::metrics::RollingAverage;
use battery::Manager;
use std::time::{Duration, Instant};
use sysinfo::System;

/// System information snapshot
#[derive(Debug, Clone)]
pub struct SystemSnapshot {
    pub cpu_usage: Vec<f32>,
    pub cpu_average: f32,
    pub memory_total: u64,
    pub memory_used: u64,
    pub memory_available: u64,
    pub memory_usage_percent: f32,
    pub uptime: u64,
    pub load_average: LoadAverage,
    pub timestamp: Instant,

    // 新增电池相关字段
    pub battery_percent: f32,
    pub is_charging: bool,
}

/// System load averages
#[derive(Debug, Clone, Default)]
pub struct LoadAverage {
    pub one_minute: f64,
    pub five_minutes: f64,
    pub fifteen_minutes: f64,
}

/// CPU information
#[derive(Debug, Clone)]
pub struct CpuInfo {
    pub name: String,
    pub usage: f32,
    pub frequency: u64,
}

/// Memory information
#[derive(Debug, Clone)]
pub struct MemoryInfo {
    pub total: u64,
    pub used: u64,
    pub available: u64,
    pub free: u64,
    pub usage_percent: f32,
}

/// System monitor with efficient caching
#[derive(Debug)]
pub struct SystemMonitor {
    system: System,
    last_update: Instant,
    update_interval: Duration,
    cpu_history: RollingAverage,
    memory_history: RollingAverage,
    last_snapshot: Option<SystemSnapshot>,
    battery_manager: Option<Manager>,
}

impl SystemMonitor {
    /// Create a new system monitor
    pub fn new(history_length: usize) -> Self {
        let mut system = System::new_all();
        system.refresh_all();
        let battery_manager = Manager::new().ok();

        Self {
            system,
            last_update: Instant::now(),
            update_interval: Duration::from_millis(1000),
            cpu_history: RollingAverage::new(history_length),
            memory_history: RollingAverage::new(history_length),
            last_snapshot: None,
            battery_manager,
        }
    }

    // 获取电池信息的方法
    fn get_battery_info(&self) -> (f32, bool) {
        if let Some(ref manager) = self.battery_manager {
            match manager.batteries() {
                Ok(batteries) => {
                    for battery_result in batteries {
                        if let Ok(battery) = battery_result {
                            let percentage = battery
                                .state_of_charge()
                                .get::<battery::units::ratio::percent>();
                            let is_charging = matches!(battery.state(), battery::State::Charging);
                            return (percentage, is_charging);
                        }
                    }
                }
                Err(_) => {}
            }
        }

        // 默认值：无电池或获取失败
        (100.0, false)
    }

    /// Update system information if needed
    pub fn update_if_needed(&mut self) -> bool {
        let now = Instant::now();
        if now.duration_since(self.last_update) >= self.update_interval {
            self.refresh();
            self.last_update = now;
            true
        } else {
            false
        }
    }

    /// Force refresh system information
    pub fn refresh(&mut self) {
        // Refresh only what we need to minimize overhead
        self.system.refresh_cpu();
        self.system.refresh_memory();

        // Create new snapshot
        let snapshot = self.create_snapshot();

        // Update history
        self.cpu_history.add(snapshot.cpu_average as f64);
        self.memory_history
            .add(snapshot.memory_usage_percent as f64);

        self.last_snapshot = Some(snapshot);
    }

    /// Create system snapshot
    fn create_snapshot(&self) -> SystemSnapshot {
        let cpu_usage: Vec<f32> = self
            .system
            .cpus()
            .iter()
            .map(|cpu| cpu.cpu_usage())
            .collect();

        let cpu_average = if cpu_usage.is_empty() {
            0.0
        } else {
            cpu_usage.iter().sum::<f32>() / cpu_usage.len() as f32
        };

        let memory_total = self.system.total_memory();
        let memory_available = self.system.available_memory();
        let memory_used = memory_total - memory_available;
        let memory_usage_percent = if memory_total > 0 {
            (memory_used as f32 / memory_total as f32) * 100.0
        } else {
            0.0
        };

        let load_average = self.get_load_average();

        // 获取电池信息
        let (battery_percent, is_charging) = self.get_battery_info();

        SystemSnapshot {
            cpu_usage,
            cpu_average,
            memory_total,
            memory_used,
            memory_available,
            memory_usage_percent,
            uptime: sysinfo::System::uptime(),
            load_average,
            timestamp: Instant::now(),
            // 新增字段
            battery_percent,
            is_charging,
        }
    }

    /// Get system load average (Unix-like systems)
    fn get_load_average(&self) -> LoadAverage {
        // On Linux, we can read from /proc/loadavg
        #[cfg(target_os = "linux")]
        {
            if let Ok(content) = std::fs::read_to_string("/proc/loadavg") {
                let parts: Vec<&str> = content.split_whitespace().collect();
                if parts.len() >= 3 {
                    return LoadAverage {
                        one_minute: parts[0].parse().unwrap_or(0.0),
                        five_minutes: parts[1].parse().unwrap_or(0.0),
                        fifteen_minutes: parts[2].parse().unwrap_or(0.0),
                    };
                }
            }
        }

        LoadAverage::default()
    }

    /// Get current system snapshot
    pub fn get_snapshot(&self) -> Option<&SystemSnapshot> {
        self.last_snapshot.as_ref()
    }

    /// Get CPU usage history
    pub fn get_cpu_history(&self) -> Vec<f64> {
        (0..self.cpu_history.len())
            .map(|_| self.cpu_history.average())
            .collect()
    }

    /// Get memory usage history
    pub fn get_memory_history(&self) -> Vec<f64> {
        (0..self.memory_history.len())
            .map(|_| self.memory_history.average())
            .collect()
    }

    /// Get individual CPU information
    pub fn get_cpu_info(&self) -> Vec<CpuInfo> {
        self.system
            .cpus()
            .iter()
            .map(|cpu| CpuInfo {
                name: cpu.name().to_string(),
                usage: cpu.cpu_usage(),
                frequency: cpu.frequency(),
            })
            .collect()
    }

    /// Get memory information
    pub fn get_memory_info(&self) -> MemoryInfo {
        let total = self.system.total_memory();
        let available = self.system.available_memory();
        let used = total - available;
        let free = self.system.free_memory();

        MemoryInfo {
            total,
            used,
            available,
            free,
            usage_percent: if total > 0 {
                (used as f32 / total as f32) * 100.0
            } else {
                0.0
            },
        }
    }

    /// Get CPU usage for chart display
    pub fn get_cpu_data_for_chart(&self) -> Vec<f64> {
        if let Some(snapshot) = &self.last_snapshot {
            snapshot
                .cpu_usage
                .iter()
                .map(|&usage| (usage / 100.0) as f64)
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Check if CPU usage is high
    pub fn is_cpu_usage_high(&self, threshold: f32) -> bool {
        if let Some(snapshot) = &self.last_snapshot {
            snapshot.cpu_average > threshold * 100.0
        } else {
            false
        }
    }

    /// Check if memory usage is high
    pub fn is_memory_usage_high(&self, threshold: f32) -> bool {
        if let Some(snapshot) = &self.last_snapshot {
            snapshot.memory_usage_percent > threshold * 100.0
        } else {
            false
        }
    }

    /// Get system uptime as formatted string
    pub fn get_uptime_string(&self) -> String {
        let uptime = if let Some(snapshot) = &self.last_snapshot {
            snapshot.uptime
        } else {
            sysinfo::System::uptime()
        };

        let days = uptime / 86400;
        let hours = (uptime % 86400) / 3600;
        let minutes = (uptime % 3600) / 60;

        if days > 0 {
            format!("{}d {}h {}m", days, hours, minutes)
        } else if hours > 0 {
            format!("{}h {}m", hours, minutes)
        } else {
            format!("{}m", minutes)
        }
    }

    /// Set update interval
    pub fn set_update_interval(&mut self, interval: Duration) {
        self.update_interval = interval;
    }

    /// Get average CPU usage over history
    pub fn get_average_cpu_usage(&self) -> f64 {
        self.cpu_history.average()
    }

    /// Get average memory usage over history
    pub fn get_average_memory_usage(&self) -> f64 {
        self.memory_history.average()
    }
}

impl Default for SystemMonitor {
    fn default() -> Self {
        Self::new(60) // Default to 60 samples
    }
}
