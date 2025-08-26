//! Application state management

use crate::audio_manager::{AudioDevice, AudioManager};
use crate::constants;
use crate::metrics::PerformanceMetrics;
use crate::system_monitor::SystemMonitor;
use shared_structures::SharedMessage;
use std::time::Instant;

/// Main application state
#[derive(Debug)]
pub struct AppState {
    /// Audio system
    pub audio_manager: AudioManager,

    /// System monitoring
    pub system_monitor: SystemMonitor,

    /// Performance metrics
    pub performance_metrics: PerformanceMetrics,

    /// UI state
    pub ui_state: UiState,

    /// Current message from shared memory
    pub current_message: Option<SharedMessage>,

    /// Application start time
    pub start_time: Instant,

    pub layout_selector_open: bool,
    pub available_layouts: Vec<LayoutInfo>,
}

#[derive(Debug, Clone)]
pub struct LayoutInfo {
    pub symbol: String,
    pub name: String,
    pub index: u32,
}

/// UI-specific state
#[derive(Debug)]
pub struct UiState {
    /// Volume control window state
    pub volume_window: VolumeWindowState,

    /// Current scale factor
    pub scale_factor: f32,

    /// Whether window needs resizing
    pub need_resize: bool,

    /// Time display format toggle
    pub show_seconds: bool,

    /// Debug window visibility
    pub show_debug_window: bool,

    /// Settings window visibility
    pub show_settings_window: bool,

    /// Last UI update time
    pub last_ui_update: Instant,

    pub button_height: f32,

    pub show_bar: bool,
    pub prev_show_bar: bool,
    pub top_y: f32,
}

/// Volume control window state
#[derive(Debug)]
pub struct VolumeWindowState {
    /// Whether the window is open
    pub open: bool,

    /// Selected device index
    pub selected_device: usize,

    /// Window position
    pub position: Option<egui::Pos2>,

    /// Last volume change time (for debouncing)
    pub last_volume_change: Instant,

    /// Volume change debounce duration
    pub volume_change_debounce: std::time::Duration,
}

impl AppState {
    /// Create new application state
    pub fn new() -> Self {
        let available_layouts = vec![
            LayoutInfo {
                symbol: "[]=".to_string(),
                name: "Tiled".to_string(),
                index: 0,
            },
            LayoutInfo {
                symbol: "><>".to_string(),
                name: "Floating".to_string(),
                index: 1,
            },
            LayoutInfo {
                symbol: "[M]".to_string(),
                name: "Monocle".to_string(),
                index: 2,
            },
        ];

        Self {
            audio_manager: AudioManager::new(),
            system_monitor: SystemMonitor::new(10),
            performance_metrics: PerformanceMetrics::new(),
            ui_state: UiState::new(),
            current_message: None,
            start_time: Instant::now(),
            layout_selector_open: false,
            available_layouts,
        }
    }

    /// Update all subsystems
    pub fn update(&mut self) {
        let now = Instant::now();

        // Update performance metrics
        self.performance_metrics.start_frame();

        // Update system monitor
        self.system_monitor.update_if_needed();

        // Update audio manager
        self.audio_manager.update_if_needed();

        // Update UI state
        self.ui_state.last_ui_update = now;
    }

    /// Get master audio device
    pub fn get_master_audio_device(&self) -> Option<&AudioDevice> {
        self.audio_manager.get_master_device()
    }

    /// Set master volume
    pub fn set_master_volume(&mut self, volume: i32) -> crate::Result<()> {
        if let Some(device) = self.audio_manager.get_master_device() {
            let device_name = device.name.clone();
            let is_muted = device.is_muted;
            self.audio_manager
                .set_volume(&device_name, volume, is_muted)
        } else {
            Err(crate::AppError::audio("No master device available"))
        }
    }

    /// Toggle master mute
    pub fn toggle_master_mute(&mut self) -> crate::Result<bool> {
        if let Some(device) = self.audio_manager.get_master_device() {
            let device_name = device.name.clone();
            self.audio_manager.toggle_mute(&device_name)
        } else {
            Err(crate::AppError::audio("No master device available"))
        }
    }

    /// Get CPU data for chart
    pub fn get_cpu_chart_data(&self) -> Vec<f64> {
        self.system_monitor.get_cpu_data_for_chart()
    }

    /// Get memory info for display
    pub fn get_memory_display_info(&self) -> (f64, f64) {
        if let Some(snapshot) = self.system_monitor.get_snapshot() {
            (
                snapshot.memory_available as f64 / 1e9, // GB
                snapshot.memory_used as f64 / 1e9,      // GB
            )
        } else {
            (0.0, 0.0)
        }
    }

    /// Check if system resources are under stress
    pub fn is_system_stressed(&self) -> bool {
        self.system_monitor.is_cpu_usage_high(0.8) || self.system_monitor.is_memory_usage_high(0.8)
    }

    /// Get application uptime
    pub fn get_uptime(&self) -> std::time::Duration {
        Instant::now().duration_since(self.start_time)
    }
}

impl UiState {
    fn new() -> Self {
        Self {
            volume_window: VolumeWindowState::new(),
            scale_factor: constants::ui::DEFAULT_SCALE_FACTOR,
            need_resize: false,
            show_seconds: false,
            show_debug_window: false,
            show_settings_window: false,
            last_ui_update: Instant::now(),
            button_height: 0.,
            show_bar: true,
            prev_show_bar: true,
            top_y: 0.0,
        }
    }

    /// Toggle volume window
    pub fn toggle_volume_window(&mut self) {
        self.volume_window.open = !self.volume_window.open;
        self.need_resize = true;
    }

    /// Toggle debug window
    pub fn toggle_debug_window(&mut self) {
        self.show_debug_window = !self.show_debug_window;
        self.need_resize = true;
    }

    /// Toggle time format
    pub fn toggle_time_format(&mut self) {
        self.show_seconds = !self.show_seconds;
    }
}

impl VolumeWindowState {
    fn new() -> Self {
        use crate::constants::intervals;

        Self {
            open: false,
            selected_device: 0,
            position: None,
            last_volume_change: Instant::now(),
            volume_change_debounce: std::time::Duration::from_millis(intervals::VOLUME_DEBOUNCE),
        }
    }

    /// Check if volume change should be applied (debouncing)
    pub fn should_apply_volume_change(&mut self) -> bool {
        let now = Instant::now();
        if now.duration_since(self.last_volume_change) > self.volume_change_debounce {
            self.last_volume_change = now;
            true
        } else {
            false
        }
    }
}
