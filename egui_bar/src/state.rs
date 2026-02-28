use egui::Color32;
use shared_structures::SharedMessage;
use std::time::{Duration, Instant};
use xbar_core::audio_manager::{AudioDevice, AudioManager};
use xbar_core::system_monitor::SystemMonitor;

use crate::theme::ui;

/// Layout information
#[derive(Debug, Clone)]
pub struct LayoutInfo {
    pub symbol: String,
    pub name: String,
    pub index: u32,
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
    pub volume_change_debounce: Duration,
}

impl VolumeWindowState {
    pub fn new() -> Self {
        Self {
            open: false,
            selected_device: 0,
            position: None,
            last_volume_change: Instant::now(),
            volume_change_debounce: Duration::from_millis(50),
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
    /// Last UI update time
    pub last_ui_update: Instant,
    /// Button height for calculations
    pub button_height: f32,
}

impl UiState {
    pub fn new() -> Self {
        Self {
            volume_window: VolumeWindowState::new(),
            scale_factor: ui::DEFAULT_SCALE_FACTOR,
            need_resize: false,
            show_seconds: false,
            show_debug_window: false,
            last_ui_update: Instant::now(),
            button_height: 0.0,
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

/// Main application state
#[derive(Debug)]
pub struct AppState {
    /// Audio system
    pub audio_manager: AudioManager,
    /// System monitoring
    pub system_monitor: SystemMonitor,
    /// UI state
    pub ui_state: UiState,
    /// Current message from shared memory
    pub current_message: Option<SharedMessage>,
    /// Layout selector state
    pub layout_selector_open: bool,
    /// Available layouts
    pub available_layouts: Vec<LayoutInfo>,
    /// Color cache for performance
    pub color_cache: Vec<Color32>,
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
            ui_state: UiState::new(),
            current_message: None,
            layout_selector_open: false,
            available_layouts,
            color_cache: Vec::new(),
        }
    }

    /// Update all subsystems
    pub fn update(&mut self) {
        let now = Instant::now();
        self.system_monitor.update_if_needed();
        self.audio_manager.update_if_needed();
        self.ui_state.last_ui_update = now;
    }

    /// Get master audio device
    pub fn get_master_audio_device(&self) -> Option<&AudioDevice> {
        self.audio_manager.get_master_device()
    }

    /// Get CPU data for chart
    pub fn get_cpu_chart_data(&self) -> Vec<f64> {
        self.system_monitor.get_cpu_data_for_chart()
    }

    /// Get memory info for display
    pub fn get_memory_display_info(&self) -> (f64, f64) {
        if let Some(snapshot) = self.system_monitor.get_snapshot() {
            (
                snapshot.memory_available as f64 / 1e9,
                snapshot.memory_used as f64 / 1e9,
            )
        } else {
            (0.0, 0.0)
        }
    }
}

/// Thread-safe shared application state
#[derive(Debug)]
pub struct SharedAppState {
    pub current_message: Option<SharedMessage>,
    pub last_update: Instant,
}

impl SharedAppState {
    pub fn new() -> Self {
        Self {
            current_message: None,
            last_update: Instant::now(),
        }
    }
}

