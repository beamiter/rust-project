//! Application state management

use crate::audio::{AudioDevice, AudioManager};
use crate::config::AppConfig;
use crate::system::SystemMonitor;
use crate::ui::theme::ThemeManager;
use crate::utils::PerformanceMetrics;
use shared_structures::SharedMessage;
use std::time::Instant;

/// Main application state
#[derive(Debug)]
pub struct AppState {
    /// Configuration
    pub config: AppConfig,

    /// Audio system
    pub audio_manager: AudioManager,

    /// System monitoring
    pub system_monitor: SystemMonitor,

    /// Theme management
    pub theme_manager: ThemeManager,

    /// Performance metrics
    pub performance_metrics: PerformanceMetrics,

    /// UI state
    pub ui_state: UiState,

    /// Current message from shared memory
    pub current_message: Option<SharedMessage>,

    /// Application start time
    pub start_time: Instant,
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

    /// Current window height
    pub current_window_height: f32,

    /// Time display format toggle
    pub show_seconds: bool,

    /// Debug window visibility
    pub show_debug_window: bool,

    /// Settings window visibility
    pub show_settings_window: bool,

    /// Last UI update time
    pub last_ui_update: Instant,
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
    pub fn new(config: AppConfig) -> Self {
        let theme_type = config.ui.theme.parse().unwrap_or_default();

        Self {
            audio_manager: AudioManager::new(),
            system_monitor: SystemMonitor::new(config.system.cpu_history_length),
            theme_manager: ThemeManager::new(theme_type),
            performance_metrics: PerformanceMetrics::new(),
            ui_state: UiState::new(),
            config,
            current_message: None,
            start_time: Instant::now(),
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
    pub fn set_master_volume(&mut self, volume: i32) -> crate::utils::Result<()> {
        if let Some(device) = self.audio_manager.get_master_device() {
            let device_name = device.name.clone();
            let is_muted = device.is_muted;
            self.audio_manager
                .set_volume(&device_name, volume, is_muted)
        } else {
            Err(crate::utils::AppError::audio("No master device available"))
        }
    }

    /// Toggle master mute
    pub fn toggle_master_mute(&mut self) -> crate::utils::Result<bool> {
        if let Some(device) = self.audio_manager.get_master_device() {
            let device_name = device.name.clone();
            self.audio_manager.toggle_mute(&device_name)
        } else {
            Err(crate::utils::AppError::audio("No master device available"))
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
        self.system_monitor
            .is_cpu_usage_high(self.config.system.cpu_warning_threshold)
            || self
                .system_monitor
                .is_memory_usage_high(self.config.system.memory_warning_threshold)
    }

    /// Get application uptime
    pub fn get_uptime(&self) -> std::time::Duration {
        Instant::now().duration_since(self.start_time)
    }

    /// Save current configuration
    pub fn save_config(&self) -> crate::utils::Result<()> {
        self.config.save()
    }
}

impl UiState {
    fn new() -> Self {
        Self {
            volume_window: VolumeWindowState::new(),
            scale_factor: 1.0,
            need_resize: true,
            current_window_height: crate::constants::ui::DEFAULT_FONT_SIZE * 2.0,
            show_seconds: false,
            show_debug_window: false,
            show_settings_window: false,
            last_ui_update: Instant::now(),
        }
    }

    /// Toggle volume window
    pub fn toggle_volume_window(&mut self) {
        self.volume_window.open = !self.volume_window.open;
        self.need_resize = true;
    }

    /// Toggle debug window - 新增方法
    pub fn toggle_debug_window(&mut self) {
        self.show_debug_window = !self.show_debug_window;
        self.need_resize = true; // 可能需要调整窗口大小
    }

    /// Toggle time format
    pub fn toggle_time_format(&mut self) {
        self.show_seconds = !self.show_seconds;
    }

    /// Request window resize
    pub fn request_resize(&mut self) {
        self.need_resize = true;
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
