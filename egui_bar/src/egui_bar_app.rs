use eframe::egui;
use egui::{
    Align, Button, Color32, FontFamily, FontId, Label, Layout, Margin, Sense, Stroke, StrokeKind,
    TextStyle, Vec2,
};
use egui_plot::{Line, Plot, PlotPoints};
use log::{debug, error, info, warn};
use shared_structures::{CommandType, SharedCommand, SharedMessage, SharedRingBuffer};
use std::collections::BTreeMap;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// Re-exports for convenience
use crate::audio_manager::{AudioDevice, AudioManager};
use crate::metrics::PerformanceMetrics;
use crate::system_monitor::SystemMonitor;

// ================================
// Constants Section
// ================================

/// UI constants
pub mod ui {
    pub const DEFAULT_FONT_SIZE: f32 = 18.0;
    pub const DEFAULT_SCALE_FACTOR: f32 = 1.0;
}

/// Color scheme
pub mod colors {
    use super::Color32;

    // Primary colors
    pub const RED: Color32 = Color32::from_rgb(255, 99, 71);
    pub const ORANGE: Color32 = Color32::from_rgb(255, 165, 0);
    pub const YELLOW: Color32 = Color32::from_rgb(255, 215, 0);
    pub const GREEN: Color32 = Color32::from_rgb(60, 179, 113);
    pub const BLUE: Color32 = Color32::from_rgb(100, 149, 237);
    pub const INDIGO: Color32 = Color32::from_rgb(75, 0, 130);
    pub const VIOLET: Color32 = Color32::from_rgb(138, 43, 226);
    pub const BROWN: Color32 = Color32::from_rgb(165, 42, 42);
    pub const GOLD: Color32 = Color32::from_rgb(255, 215, 0);
    pub const MAGENTA: Color32 = Color32::from_rgb(255, 0, 255);
    pub const CYAN: Color32 = Color32::from_rgb(0, 206, 209);
    pub const SILVER: Color32 = Color32::from_rgb(192, 192, 192);
    pub const OLIVE_GREEN: Color32 = Color32::from_rgb(128, 128, 0);
    pub const ROYALBLUE: Color32 = Color32::from_rgb(65, 105, 225);
    pub const WHEAT: Color32 = Color32::from_rgb(245, 222, 179);

    // System status colors
    pub const CPU_LOW: Color32 = GREEN;
    pub const CPU_MEDIUM: Color32 = YELLOW;
    pub const CPU_HIGH: Color32 = ORANGE;
    pub const CPU_CRITICAL: Color32 = RED;

    pub const MEMORY_AVAILABLE: Color32 = CYAN;
    pub const MEMORY_USED: Color32 = SILVER;

    // Tag colors for workspace indicators
    pub const TAG_COLORS: [Color32; 9] = [
        Color32::from_rgb(0xFF, 0x6B, 0x6B), // Red
        Color32::from_rgb(0x4E, 0xCD, 0xC4), // Cyan
        Color32::from_rgb(0x45, 0xB7, 0xD1), // Blue
        Color32::from_rgb(0x96, 0xCE, 0xB4), // Green
        Color32::from_rgb(0xFE, 0xCA, 0x57), // Yellow
        Color32::from_rgb(0xFF, 0x9F, 0xF3), // Pink
        Color32::from_rgb(0x54, 0xA0, 0xFF), // Light Blue
        Color32::from_rgb(0x5F, 0x27, 0xCD), // Purple
        Color32::from_rgb(0x00, 0xD2, 0xD3), // Teal
    ];

    // UI accent colors
    pub const ACCENT_PRIMARY: Color32 = BLUE;
    pub const ACCENT_SECONDARY: Color32 = CYAN;
    pub const WARNING: Color32 = ORANGE;
    pub const ERROR: Color32 = RED;
    pub const SUCCESS: Color32 = GREEN;

    // Battery related colors
    pub const BATTERY_HIGH: Color32 = Color32::from_rgb(76, 175, 80); // Green
    pub const BATTERY_MEDIUM: Color32 = Color32::from_rgb(255, 193, 7); // Yellow
    pub const BATTERY_LOW: Color32 = Color32::from_rgb(244, 67, 54); // Red
    pub const CHARGING: Color32 = Color32::from_rgb(33, 150, 243); // Blue
    pub const UNAVAILABLE: Color32 = Color32::from_rgb(158, 158, 158); // Gray
}

/// Icons and symbols
pub mod icons {
    // Workspace tag icons
    pub const TAG_ICONS: [&str; 9] = ["🏠", "💻", "🌐", "🎵", "📁", "🎮", "📧", "🔧", "📊"];

    // Audio icons
    pub const VOLUME_MUTED: &str = "🔇";
    pub const VOLUME_LOW: &str = "🔈";
    pub const VOLUME_MEDIUM: &str = "🔉";
    pub const VOLUME_HIGH: &str = "🔊";

    // System icons
    pub const CPU_ICON: &str = "🔥";
    pub const MEMORY_ICON: &str = "💾";
    pub const SCREENSHOT_ICON: &str = "📸";
    pub const SETTINGS_ICON: &str = "⚙️";

    // Monitor numbers
    pub const MONITOR_NUMBERS: [&str; 2] = ["󰎡", "󰎤"];
}

/// Font families to try loading
pub const FONT_FAMILIES: &[&str] = &[
    "Noto Sans CJK SC",
    "Noto Sans CJK TC",
    "SauceCodeProNerdFont",
];

/// Application metadata
pub mod app {
    pub const DEFAULT_LOG_LEVEL: &str = "info";
    pub const LOG_FILE_MAX_SIZE: u64 = 10_000_000; // 10MB
    pub const LOG_FILE_MAX_COUNT: usize = 5;
    pub const HEARTBEAT_TIMEOUT_SECS: u64 = 5;
}

// Helps control CPU plot per-core point drawing on many-core systems
const PER_CORE_POINTS_THRESHOLD: usize = 32;

// ================================
// Application State Section
// ================================

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
    pub volume_change_debounce: std::time::Duration,
}

impl VolumeWindowState {
    fn new() -> Self {
        Self {
            open: false,
            selected_device: 0,
            position: None,
            last_volume_change: Instant::now(),
            volume_change_debounce: std::time::Duration::from_millis(50),
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
    /// Settings window visibility
    pub show_settings_window: bool,
    /// Last UI update time
    pub last_ui_update: Instant,
    /// Button height for calculations
    pub button_height: f32,
}

impl UiState {
    fn new() -> Self {
        Self {
            volume_window: VolumeWindowState::new(),
            scale_factor: ui::DEFAULT_SCALE_FACTOR,
            need_resize: false,
            show_seconds: false,
            show_debug_window: false,
            show_settings_window: false,
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
    /// Performance metrics
    pub performance_metrics: PerformanceMetrics,
    /// UI state
    pub ui_state: UiState,
    /// Current message from shared memory
    pub current_message: Option<SharedMessage>,
    /// Application start time
    pub start_time: Instant,
    /// Layout selector state
    pub layout_selector_open: bool,
    /// Available layouts
    pub available_layouts: Vec<LayoutInfo>,
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

// ================================
// Shared State Section
// ================================

/// Thread-safe shared application state
#[derive(Debug)]
pub struct SharedAppState {
    pub current_message: Option<SharedMessage>,
    pub last_update: Instant,
}

impl SharedAppState {
    fn new() -> Self {
        Self {
            current_message: None,
            last_update: Instant::now(),
        }
    }
}

// ================================
// Main Application Section
// ================================

/// Main egui application
pub struct EguiBarApp {
    /// Application state
    state: AppState,
    /// Thread-safe shared state
    shared_state: Arc<Mutex<SharedAppState>>,
    /// Color cache for performance
    color_cache: Vec<Color32>,
    /// Shared buffer for communication
    shared_buffer_rc: Option<Arc<SharedRingBuffer>>,
}

impl EguiBarApp {
    /// Create new application instance
    pub fn new(cc: &eframe::CreationContext<'_>, shared_path: String) -> crate::Result<Self> {
        cc.egui_ctx.set_theme(egui::Theme::Light);
        let state = AppState::new();
        let shared_state = Arc::new(Mutex::new(SharedAppState::new()));

        #[cfg(feature = "debug_mode")]
        {
            cc.egui_ctx.set_debug_on_hover(true);
        }

        // Setup fonts and UI
        Self::setup_custom_fonts(&cc.egui_ctx)?;
        Self::configure_text_styles(&cc.egui_ctx);

        let shared_buffer_rc =
            SharedRingBuffer::create_shared_ring_buffer_aux(&shared_path).map(Arc::new);

        // Start background tasks (std::thread, no tokio)
        Self::start_background_tasks(
            &shared_state,
            &cc.egui_ctx,
            shared_buffer_rc.clone(),
        );

        Ok(Self {
            state,
            shared_state,
            color_cache: Vec::new(),
            shared_buffer_rc,
        })
    }

    /// Start background worker tasks (no tokio)
    fn start_background_tasks(
        shared_state: &Arc<Mutex<SharedAppState>>,
        egui_ctx: &egui::Context,
        shared_buffer_rc: Option<Arc<SharedRingBuffer>>,
    ) {
        // Shared memory worker
        {
            let shared_state_clone = Arc::clone(shared_state);
            let egui_ctx_clone = egui_ctx.clone();
            if let Some(shared_buffer) = shared_buffer_rc {
                thread::spawn(move || {
                    Self::shared_memory_worker(shared_buffer, shared_state_clone, egui_ctx_clone);
                });
            }
        }

        // Periodic update task
        {
            let egui_ctx_clone = egui_ctx.clone();
            thread::spawn(move || {
                Self::periodic_update_task(egui_ctx_clone);
            });
        }
    }

    /// Shared memory worker task (blocking loop)
    fn shared_memory_worker(
        shared_buffer: Arc<SharedRingBuffer>,
        shared_state: Arc<Mutex<SharedAppState>>,
        egui_ctx: egui::Context,
    ) {
        info!("Starting shared memory worker task");
        let mut prev_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        loop {
            match shared_buffer.wait_for_message(Some(Duration::from_secs(2))) {
                Ok(true) => {
                    if let Ok(Some(message)) = shared_buffer.try_read_latest_message() {
                        if prev_timestamp != message.timestamp.into() {
                            prev_timestamp = message.timestamp.into();
                            if let Ok(mut state) = shared_state.lock() {
                                let need_update = state
                                    .current_message
                                    .as_ref()
                                    .map(|m| m.timestamp != message.timestamp)
                                    .unwrap_or(true);
                                if need_update {
                                    info!("current_message: {:?}", message);
                                    state.current_message = Some(message);
                                    state.last_update = Instant::now();
                                    egui_ctx.request_repaint();
                                }
                            } else {
                                warn!("Failed to lock shared state for message update");
                            }
                        }
                    }
                }
                Ok(false) => {
                    debug!("[notifier] Wait for message timed out.");
                }
                Err(e) => {
                    error!("[notifier] Wait for message failed: {}", e);
                    break;
                }
            }
        }

        info!("Shared memory worker task exiting");
    }

    /// Periodic update task for time display (no tokio)
    fn periodic_update_task(egui_ctx: egui::Context) {
        info!("Starting periodic update task");
        let mut last_second = chrono::Local::now().timestamp();

        loop {
            thread::sleep(Duration::from_millis(500));
            let current_second = chrono::Local::now().timestamp();
            if current_second != last_second {
                last_second = current_second;
                egui_ctx.request_repaint();
            }
        }
    }

    /// Setup custom fonts
    fn setup_custom_fonts(ctx: &egui::Context) -> crate::Result<()> {
        use font_kit::family_name::FamilyName;
        use font_kit::properties::Properties;
        use font_kit::source::SystemSource;
        use std::collections::HashSet;

        info!("Loading system fonts...");
        let mut fonts = egui::FontDefinitions::default();
        let system_source = SystemSource::new();

        let original_proportional = fonts
            .families
            .get(&FontFamily::Proportional)
            .cloned()
            .unwrap_or_default();
        let original_monospace = fonts
            .families
            .get(&FontFamily::Monospace)
            .cloned()
            .unwrap_or_default();

        let mut loaded_fonts = Vec::new();
        let mut seen_fonts = HashSet::new();

        for &font_name in FONT_FAMILIES {
            if fonts.font_data.contains_key(font_name) || seen_fonts.contains(font_name) {
                info!("Font {} already loaded, skipping", font_name);
                continue;
            }

            info!("Attempting to load font: {}", font_name);

            let font_result = system_source
                .select_best_match(
                    &[FamilyName::Title(font_name.to_string())],
                    &Properties::new(),
                )
                .and_then(|handle| {
                    handle
                        .load()
                        .map_err(|_| font_kit::error::SelectionError::NotFound)
                })
                .and_then(|font| {
                    font.copy_font_data()
                        .ok_or(font_kit::error::SelectionError::NotFound)
                });

            match font_result {
                Ok(font_data) => {
                    let font_key = font_name.to_string();
                    fonts.font_data.insert(
                        font_key.clone(),
                        egui::FontData::from_owned(font_data.to_vec()).into(),
                    );
                    loaded_fonts.push(font_key);
                    seen_fonts.insert(font_name);
                    info!("Successfully loaded font: {}", font_name);
                }
                Err(e) => {
                    info!("Failed to load font {}: {}", font_name, e);
                }
            }
        }

        if !loaded_fonts.is_empty() {
            Self::update_font_families(
                &mut fonts,
                loaded_fonts,
                original_proportional,
                original_monospace,
            );
            info!(
                "Font setup completed with {} custom fonts",
                fonts.font_data.len() - 2
            );
        } else {
            info!("No custom fonts loaded, using default configuration");
        }

        ctx.set_fonts(fonts);
        Ok(())
    }

    /// Update font families
    fn update_font_families(
        fonts: &mut egui::FontDefinitions,
        loaded_fonts: Vec<String>,
        original_proportional: Vec<String>,
        original_monospace: Vec<String>,
    ) {
        let new_proportional = [loaded_fonts.clone(), original_proportional].concat();
        let new_monospace = [loaded_fonts.clone(), original_monospace].concat();

        fonts
            .families
            .insert(FontFamily::Proportional, new_proportional);
        fonts.families.insert(FontFamily::Monospace, new_monospace);

        info!("Updated font families:");
        info!(
            "  Proportional: {:?}",
            fonts.families.get(&FontFamily::Proportional)
        );
        info!(
            "  Monospace: {:?}",
            fonts.families.get(&FontFamily::Monospace)
        );
    }

    /// Configure text styles
    pub fn configure_text_styles(ctx: &egui::Context) {
        ctx.all_styles_mut(|style| {
            let base_font_size = ui::DEFAULT_FONT_SIZE;
            let text_styles: BTreeMap<TextStyle, FontId> = [
                (
                    TextStyle::Small,
                    FontId::new(base_font_size * 0.8, FontFamily::Monospace),
                ),
                (
                    TextStyle::Body,
                    FontId::new(base_font_size, FontFamily::Monospace),
                ),
                (
                    TextStyle::Monospace,
                    FontId::new(base_font_size, FontFamily::Monospace),
                ),
                (
                    TextStyle::Button,
                    FontId::new(base_font_size, FontFamily::Monospace),
                ),
                (
                    TextStyle::Heading,
                    FontId::new(base_font_size * 1.5, FontFamily::Monospace),
                ),
            ]
            .into();

            style.text_styles = text_styles;
            style.spacing.window_margin = Margin::ZERO;
            style.spacing.button_padding = Vec2::new(2.0, 1.0);
        });
    }

    /// Get current message from shared state
    fn get_current_message(&self) -> Option<SharedMessage> {
        self.shared_state
            .lock()
            .ok()
            .and_then(|state| state.current_message.clone())
    }

    /// Calculate target window height
    fn calculate_target_height(&self, _ui: &egui::Ui) -> f32 {
        if let Some(message) = self.get_current_message() {
            let monitor_info = &message.monitor_info;
            if self.state.ui_state.volume_window.open || self.state.ui_state.show_debug_window {
                return monitor_info.monitor_height as f32 * 0.618;
            }
        }
        40.0
    }

    /// Adjust window size and position
    fn adjust_window(&mut self, ctx: &egui::Context, ui: &egui::Ui) {
        if self.state.ui_state.need_resize {
            let target_height = self.calculate_target_height(ui);
            let viewport_info = ctx.input(|i| i.viewport().clone());
            info!("viewport_info: {:?}", viewport_info);

            if let Some(outer_rect) = viewport_info.outer_rect {
                let target_width = outer_rect.width();
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(
                    target_width,
                    target_height,
                )));
                info!("Window adjusted size: {}x{}", target_width, target_height);
            }

            self.state.ui_state.need_resize = false;
        }
    }

    /// Main UI drawing function
    fn draw_main_ui(&mut self, ui: &mut egui::Ui) {
        // Update current message
        if let Some(message) = self.get_current_message() {
            self.state.current_message = Some(message);
        }

        ui.horizontal_centered(|ui| {
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                self.draw_workspace_panel(ui);
            });

            ui.columns(2, |ui| {
                ui[0].with_layout(Layout::left_to_right(Align::Center), |_ui| {});
                ui[1].with_layout(Layout::right_to_left(Align::Center), |ui| {
                    self.draw_controller_info_panel(ui);
                    self.draw_system_info_panel(ui);
                });
            });
        });
    }

    /// Draw workspace panel with tags and layout selector
    pub fn draw_workspace_panel(&mut self, ui: &mut egui::Ui) {
        let bold_thickness = 2.5;
        let light_thickness = 1.5;
        let monitor_info = self
            .state
            .current_message
            .as_ref()
            .map(|m| m.monitor_info)
            .unwrap_or_default();

        // Draw tag icons as buttons
        for (index, &tag_icon) in icons::TAG_ICONS.iter().enumerate() {
            let tag_color = colors::TAG_COLORS[index];
            let tag_bit = 1 << index;

            let rich_text = egui::RichText::new(tag_icon).monospace();

            let mut is_urg = false;
            let mut is_filled = false;
            let mut is_selected = false;
            let mut tooltip = format!("Tag {}", index + 1);
            let mut button_bg_color = Color32::TRANSPARENT;

            if let Some(tag_status) = monitor_info.tag_status_vec.get(index) {
                if tag_status.is_urg {
                    tooltip.push_str(" (urgent)");
                    is_urg = true;
                    button_bg_color = Color32::RED;
                } else if tag_status.is_filled {
                    is_filled = true;
                    tooltip.push_str(" (has windows)");
                    button_bg_color = Color32::from_rgba_premultiplied(
                        tag_color.r(),
                        tag_color.g(),
                        tag_color.b(),
                        255,
                    );
                } else if tag_status.is_selected {
                    tooltip.push_str(" (current)");
                    is_selected = true;
                    button_bg_color = Color32::from_rgba_premultiplied(
                        tag_color.r(),
                        tag_color.g(),
                        tag_color.b(),
                        210,
                    );
                } else if tag_status.is_occ {
                    button_bg_color = Color32::from_rgba_premultiplied(
                        tag_color.r(),
                        tag_color.g(),
                        tag_color.b(),
                        180,
                    );
                }
            }

            let button = Button::new(rich_text)
                .min_size(Vec2::new(36.0, 24.0))
                .fill(button_bg_color);

            let label_response = ui.add(button);
            let rect = label_response.rect;
            self.state.ui_state.button_height = rect.height();

            // Draw border decorations
            if is_urg {
                ui.painter().rect_stroke(
                    rect,
                    1.0,
                    Stroke::new(bold_thickness, colors::VIOLET),
                    StrokeKind::Inside,
                );
            } else if is_filled {
                ui.painter().rect_stroke(
                    rect,
                    1.0,
                    Stroke::new(bold_thickness, tag_color),
                    StrokeKind::Inside,
                );
            } else if is_selected {
                ui.painter().rect_stroke(
                    rect,
                    1.0,
                    Stroke::new(light_thickness, tag_color),
                    StrokeKind::Inside,
                );
            }

            // Handle interactions
            self.handle_tag_interactions(&label_response, tag_bit, index);

            // Hover effects and tooltips
            if label_response.hovered() {
                ui.painter().rect_stroke(
                    rect.expand(1.0),
                    1.0,
                    Stroke::new(bold_thickness, tag_color),
                    StrokeKind::Inside,
                );
                label_response.on_hover_text(tooltip);
            }
        }

        self.render_layout_section(ui, &monitor_info.get_ltsymbol());
    }

    /// Handle tag interaction events
    fn handle_tag_interactions(
        &self,
        label_response: &egui::Response,
        tag_bit: u32,
        _tag_index: usize,
    ) {
        if label_response.clicked() {
            info!("{} clicked", tag_bit);
            self.send_tag_command(tag_bit, true);
        }

        if label_response.secondary_clicked() {
            info!("{} secondary_clicked", tag_bit);
            self.send_tag_command(tag_bit, false);
        }
    }

    /// Send tag-related commands
    fn send_tag_command(&self, tag_bit: u32, is_view: bool) {
        if let Some(ref message) = self.state.current_message {
            let monitor_id = message.monitor_info.monitor_num;

            let command = if is_view {
                SharedCommand::view_tag(tag_bit, monitor_id)
            } else {
                SharedCommand::toggle_tag(tag_bit, monitor_id)
            };

            if let Some(shared_buffer) = &self.shared_buffer_rc {
                match shared_buffer.send_command(command) {
                    Ok(true) => info!("Sent command: {:?} by shared_buffer", command),
                    Ok(false) => warn!("Command buffer full, command dropped"),
                    Err(e) => error!("Failed to send command: {}", e),
                }
            }
        }
    }

    /// Send layout change command
    fn send_layout_command(&mut self, layout_index: u32) {
        if let Some(ref message) = self.state.current_message {
            let monitor_id = message.monitor_info.monitor_num;
            let command = SharedCommand::new(CommandType::SetLayout, layout_index, monitor_id);

            if let Some(shared_buffer) = &self.shared_buffer_rc {
                match shared_buffer.send_command(command) {
                    Ok(true) => info!("Sent command: {:?} by shared_buffer", command),
                    Ok(false) => warn!("Command buffer full, command dropped"),
                    Err(e) => error!("Failed to send command: {}", e),
                }
            }
        }
    }

    /// Render layout section with selector
    fn render_layout_section(&mut self, ui: &mut egui::Ui, layout_symbol: &str) {
        ui.separator();

        let main_layout_button = ui.add(
            egui::Button::new(egui::RichText::new(layout_symbol).color(
                if self.state.layout_selector_open {
                    colors::SUCCESS
                } else {
                    colors::ERROR
                },
            ))
            .small(),
        );

        if main_layout_button.clicked() {
            info!("Layout button clicked, toggling selector");
            self.state.layout_selector_open = !self.state.layout_selector_open;
        }

        if self.state.layout_selector_open {
            ui.separator();

            for layout in self.state.available_layouts.clone() {
                let is_current = layout.symbol == layout_symbol;

                let layout_option_button = ui.add(
                    egui::Button::new(egui::RichText::new(&layout.symbol).color(if is_current {
                        colors::SUCCESS
                    } else {
                        colors::ROYALBLUE
                    }))
                    .small()
                    .selected(is_current),
                );

                if layout_option_button.clicked() && !is_current {
                    info!("Layout option clicked: {} ({})", layout.name, layout.symbol);
                    self.send_layout_command(layout.index);
                    self.state.layout_selector_open = false;
                }

                let hover_text = format!("Switch layout to: {}", layout.name);
                layout_option_button.on_hover_text(hover_text);
            }
        }
    }

    /// Draw controller information panel
    pub fn draw_controller_info_panel(&mut self, ui: &mut egui::Ui) {
        self.draw_battery_info(ui);
        self.draw_volume_button(ui);
        self.draw_debug_button(ui);
        self.draw_time_display(ui);
        self.draw_screenshot_button(ui);
        self.draw_monitor_number(ui);
    }

    /// Draw battery information
    fn draw_battery_info(&self, ui: &mut egui::Ui) {
        if let Some(snapshot) = self.state.system_monitor.get_snapshot() {
            let battery_percent = snapshot.battery_percent;
            let is_charging = snapshot.is_charging;

            let battery_color = match battery_percent {
                p if p > 50.0 => colors::BATTERY_HIGH,
                p if p > 20.0 => colors::BATTERY_MEDIUM,
                _ => colors::BATTERY_LOW,
            };

            let battery_icon = if is_charging {
                "🔌"
            } else {
                match battery_percent {
                    p if p > 75.0 => "🔋",
                    p if p > 50.0 => "🔋",
                    p if p > 25.0 => "🪫",
                    _ => "🪫",
                }
            };

            ui.label(egui::RichText::new(battery_icon).color(battery_color));
            ui.label(egui::RichText::new(format!("{:.0}%", battery_percent)).color(battery_color));

            if battery_percent < 20.0 && !is_charging {
                ui.label(egui::RichText::new("⚠️").color(colors::WARNING));
            }

            if is_charging {
                ui.label(egui::RichText::new("⚡").color(colors::CHARGING));
            }
        } else {
            ui.label(egui::RichText::new("❓").color(colors::UNAVAILABLE));
        }
    }

    /// Draw volume control button
    fn draw_volume_button(&mut self, ui: &mut egui::Ui) {
        let (volume_icon, tooltip) = if let Some(device) = self.state.get_master_audio_device() {
            let icon = if device.is_muted || device.volume == 0 {
                icons::VOLUME_MUTED
            } else if device.volume < 30 {
                icons::VOLUME_LOW
            } else if device.volume < 70 {
                icons::VOLUME_MEDIUM
            } else {
                icons::VOLUME_HIGH
            };

            let tooltip = format!(
                "{}: {}%{}",
                device.description,
                device.volume,
                if device.is_muted { " (muted)" } else { "" }
            );

            (icon, tooltip)
        } else {
            (icons::VOLUME_MUTED, "No audio device".to_string())
        };

        let label_response = ui.add(Button::new(volume_icon));
        if label_response.clicked() {
            self.state.ui_state.toggle_volume_window();
        }
        label_response.on_hover_text(tooltip);
    }

    /// Draw debug control button
    fn draw_debug_button(&mut self, ui: &mut egui::Ui) {
        let (debug_icon, tooltip) = if self.state.ui_state.show_debug_window {
            ("󰱭", "Close debug window")
        } else {
            ("🔍", "Open debug window")
        };

        let label_response = ui.add(Button::new(debug_icon).sense(Sense::click()));
        if label_response.clicked() {
            self.state.ui_state.toggle_debug_window();
        }
        label_response.on_hover_text(tooltip);
    }

    /// Draw time display
    fn draw_time_display(&mut self, ui: &mut egui::Ui) {
        let format_str = if self.state.ui_state.show_seconds {
            "%Y-%m-%d %H:%M:%S"
        } else {
            "%Y-%m-%d %H:%M"
        };

        let current_time = chrono::Local::now().format(format_str).to_string();

        if ui
            .selectable_label(
                true,
                egui::RichText::new(current_time)
                    .color(colors::GREEN)
                    .small(),
            )
            .clicked()
        {
            self.state.ui_state.toggle_time_format();
        }
    }

    /// Draw screenshot button
    fn draw_screenshot_button(&mut self, ui: &mut egui::Ui) {
        let label_response = ui.add(Button::new(icons::SCREENSHOT_ICON));

        if label_response.clicked() {
            let _ = Command::new("flameshot").arg("gui").spawn();
        }

        label_response.on_hover_text(format!(
            "Screenshot (flameshot)\nScale: {:.2}",
            self.state.ui_state.scale_factor
        ));
    }

    /// Draw monitor number
    fn draw_monitor_number(&mut self, ui: &mut egui::Ui) {
        if let Some(ref message) = self.state.current_message {
            let monitor_num = (message.monitor_info.monitor_num as usize).min(1);
            ui.add(Label::new(
                egui::RichText::new(format!("{}", icons::MONITOR_NUMBERS[monitor_num])).strong(),
            ));
        }
    }

    /// Draw system information panel
    pub fn draw_system_info_panel(&mut self, ui: &mut egui::Ui) {
        self.ensure_color_cache();
        self.draw_memory_info(ui);
        self.draw_cpu_chart(ui);
    }

    /// Draw memory information
    fn draw_memory_info(&self, ui: &mut egui::Ui) {
        let (available_gb, used_gb) = self.state.get_memory_display_info();

        ui.label(
            egui::RichText::new(format!("{:.1}G", available_gb)).color(colors::MEMORY_AVAILABLE),
        );

        ui.label(egui::RichText::new(format!("{:.1}G", used_gb)).color(colors::MEMORY_USED));

        if let Some(snapshot) = self.state.system_monitor.get_snapshot() {
            if snapshot.memory_usage_percent > 0.8 * 100.0 {
                ui.label("⚠️");
            }
        }
        ui.separator();
    }

    /// Draw CPU usage chart
    fn draw_cpu_chart(&mut self, ui: &mut egui::Ui) {
        let reset_view = ui.add(Button::new("🔄"));

        if let Some(snapshot) = self.state.system_monitor.get_snapshot() {
            let cpu_color = self.get_cpu_color(snapshot.cpu_average as f64 / 100.0);
            ui.label(
                egui::RichText::new(format!("{}%", snapshot.cpu_average as i32)).color(cpu_color),
            );

            if snapshot.cpu_average > 0.8 * 100.0 {
                ui.label(egui::RichText::new("🔥").color(colors::WARNING));
            }
        }

        let cpu_data = self.state.get_cpu_chart_data();
        if cpu_data.is_empty() {
            return;
        }

        let available_width = ui.available_width();
        let chart_height = ui.available_height();
        let chart_width = available_width;

        let mut plot = Plot::new("cpu_usage_chart")
            .include_y(0.0)
            .include_y(1.2)
            .x_axis_formatter(|_, _| String::new())
            .y_axis_formatter(|_, _| String::new())
            .show_axes([false, false])
            .show_background(false)
            .width(chart_width)
            .height(chart_height);

        if reset_view.clicked() {
            plot = plot.reset();
        }

        plot.show(ui, |plot_ui| {
            let plot_points: Vec<[f64; 2]> = cpu_data
                .iter()
                .enumerate()
                .map(|(i, &usage)| [i as f64, usage])
                .collect();

            if !plot_points.is_empty() {
                let line = Line::new("CPU Usage", PlotPoints::from(plot_points))
                    .color(self.get_average_cpu_color(&cpu_data))
                    .width(1.0);
                plot_ui.line(line);

                let draw_points = cpu_data.len() <= PER_CORE_POINTS_THRESHOLD;
                if draw_points {
                    for (core_idx, &usage) in cpu_data.iter().enumerate() {
                        let color = self.get_cpu_color(usage);
                        let points = vec![[core_idx as f64, usage]];

                        let core_point = egui_plot::Points::new(
                            format!("Core {}", core_idx),
                            PlotPoints::from(points),
                        )
                        .color(color)
                        .radius(2.0)
                        .shape(egui_plot::MarkerShape::Circle);

                        plot_ui.points(core_point);
                    }
                }

                if cpu_data.len() > 1 {
                    let avg_usage = cpu_data.iter().sum::<f64>() / cpu_data.len() as f64;
                    let avg_points: Vec<[f64; 2]> =
                        (0..cpu_data.len()).map(|i| [i as f64, avg_usage]).collect();

                    let avg_line = Line::new("Average", PlotPoints::from(avg_points))
                        .color(Color32::WHITE)
                        .width(1.0)
                        .style(egui_plot::LineStyle::Dashed { length: 5.0 });

                    plot_ui.line(avg_line);
                }
            }
        });
    }

    /// Get CPU color based on usage
    fn get_cpu_color(&self, usage: f64) -> Color32 {
        let usage = usage.clamp(0.0, 1.0);

        if usage < 0.3 {
            colors::CPU_LOW
        } else if usage < 0.6 {
            colors::CPU_MEDIUM
        } else if usage < 0.8 {
            colors::CPU_HIGH
        } else {
            colors::CPU_CRITICAL
        }
    }

    /// Get average CPU color
    fn get_average_cpu_color(&self, cpu_data: &[f64]) -> Color32 {
        if cpu_data.is_empty() {
            return colors::CPU_LOW;
        }

        let avg_usage = cpu_data.iter().sum::<f64>() / cpu_data.len() as f64;
        self.get_cpu_color(avg_usage)
    }

    /// Ensure color cache is initialized
    fn ensure_color_cache(&mut self) {
        if self.color_cache.is_empty() {
            self.color_cache = (0..=100)
                .map(|i| {
                    let usage = i as f64 / 100.0;
                    self.get_cpu_color(usage)
                })
                .collect();
        }
    }

    /// Draw volume control window
    pub fn draw_volume_control_window(&mut self, ctx: &egui::Context) {
        if !self.state.ui_state.volume_window.open {
            return;
        }

        let mut window_open = true;

        egui::Window::new("🔊 Volume Control")
            .collapsible(false)
            .resizable(false)
            .default_width(320.0)
            .default_pos(
                self.state
                    .ui_state
                    .volume_window
                    .position
                    .unwrap_or_else(|| {
                        let screen_rect = ctx.screen_rect();
                        egui::pos2(
                            screen_rect.center().x - 160.0,
                            screen_rect.center().y - 150.0,
                        )
                    }),
            )
            .open(&mut window_open)
            .show(ctx, |ui| {
                if let Some(rect) = ctx.memory(|mem| mem.area_rect(ui.id())) {
                    self.state.ui_state.volume_window.position = Some(rect.left_top());
                }

                self.draw_volume_content(ui);

                ui.horizontal(|ui| {
                    if ui.button("🔧 Advanced Mixer").clicked() {
                        let _ = std::process::Command::new("terminator")
                            .args(["-e", "alsamixer"])
                            .spawn();
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("✖ Close").clicked() {
                            self.state.ui_state.toggle_volume_window();
                        }
                    });
                });
            });

        if !window_open || ctx.input(|i| i.viewport().close_requested()) {
            self.state.ui_state.toggle_volume_window();
        }
    }

    /// Draw volume control content
    fn draw_volume_content(&mut self, ui: &mut egui::Ui) {
        let devices: Vec<crate::audio_manager::AudioDevice> =
            self.state.audio_manager.get_devices().to_vec();

        if devices.is_empty() {
            ui.add(Label::new("❌ No controllable audio device found"));
            return;
        }

        let controllable_devices: Vec<(usize, crate::audio_manager::AudioDevice)> = devices
            .into_iter()
            .enumerate()
            .filter(|(_, d)| d.has_volume_control || d.has_switch_control)
            .collect();

        if controllable_devices.is_empty() {
            ui.add(Label::new("❌ No controllable audio device found"));
            return;
        }

        self.draw_device_selector(ui, &controllable_devices);
        ui.add_space(10.0);

        if let Some((_, device)) =
            controllable_devices.get(self.state.ui_state.volume_window.selected_device)
        {
            self.draw_device_controls(ui, device);
        }
    }

    /// Draw device selector
    fn draw_device_selector(
        &mut self,
        ui: &mut egui::Ui,
        controllable_devices: &[(usize, crate::audio_manager::AudioDevice)],
    ) {
        ui.horizontal(|ui| {
            ui.add(Label::new("🎵 Device:"));

            if self.state.ui_state.volume_window.selected_device >= controllable_devices.len() {
                self.state.ui_state.volume_window.selected_device = 0;
            }

            let current_selection =
                &controllable_devices[self.state.ui_state.volume_window.selected_device];

            egui::ComboBox::from_id_salt("audio_device_selector")
                .selected_text(&current_selection.1.description)
                .width(200.0)
                .show_ui(ui, |ui| {
                    for (idx, (_, device)) in controllable_devices.iter().enumerate() {
                        if ui
                            .selectable_label(
                                self.state.ui_state.volume_window.selected_device == idx,
                                &device.description,
                            )
                            .clicked()
                        {
                            self.state.ui_state.volume_window.selected_device = idx;
                        }
                    }
                });
        });
    }

    /// Draw device controls
    fn draw_device_controls(
        &mut self,
        ui: &mut egui::Ui,
        device: &crate::audio_manager::AudioDevice,
    ) {
        let device_name = device.name.clone();
        let mut current_volume = device.volume;
        let is_muted = device.is_muted;

        if device.has_volume_control {
            ui.horizontal(|ui| {
                ui.add(Label::new("🔊 Volume:"));

                if device.has_switch_control {
                    let mute_icon = if is_muted {
                        icons::VOLUME_MUTED
                    } else {
                        icons::VOLUME_HIGH
                    };
                    let mute_btn = ui.button(mute_icon);

                    if mute_btn.clicked() {
                        if let Err(e) = self.state.audio_manager.toggle_mute(&device_name) {
                            error!("Failed to toggle mute: {}", e);
                        }
                    }

                    mute_btn.on_hover_text(if is_muted { "Unmute" } else { "Mute" });
                }

                ui.label(format!("{}%", current_volume));
            });

            let slider_response = ui.add(
                egui::Slider::new(&mut current_volume, 0..=100)
                    .show_value(false)
                    .text(""),
            );

            if slider_response.changed()
                && self
                    .state
                    .ui_state
                    .volume_window
                    .should_apply_volume_change()
            {
                if let Err(e) =
                    self.state
                        .audio_manager
                        .set_volume(&device_name, current_volume, is_muted)
                {
                    error!("Failed to set volume: {}", e);
                }
            }
        } else if device.has_switch_control {
            ui.horizontal(|ui| {
                let btn_text = if is_muted {
                    "🔴 Disabled"
                } else {
                    "🟢 Enabled"
                };
                let btn_color = if is_muted {
                    colors::ERROR
                } else {
                    colors::SUCCESS
                };

                if ui
                    .add(egui::Button::new(btn_text).fill(btn_color))
                    .clicked()
                {
                    if let Err(e) = self.state.audio_manager.toggle_mute(&device_name) {
                        error!("Failed to toggle mute: {}", e);
                    }
                }
            });
        } else {
            ui.add(Label::new("❌ No available controls for this device"));
        }

        ui.separator();
        ui.horizontal(|ui| {
            ui.add(Label::new(format!("📋 Type: {:?}", device.device_type)));
            ui.add(Label::new(format!(
                "📹 Controls: {}",
                if device.has_volume_control && device.has_switch_control {
                    "Volume + Switch"
                } else if device.has_volume_control {
                    "Volume only"
                } else if device.has_switch_control {
                    "Switch only"
                } else {
                    "None"
                }
            )));
        });
    }

    /// Draw debug display window
    pub fn draw_debug_display_window(&mut self, ctx: &egui::Context) {
        if !self.state.ui_state.show_debug_window {
            return;
        }

        let mut window_open = true;

        egui::Window::new("🐛 Debug Info")
            .collapsible(false)
            .resizable(true)
            .default_width(400.0)
            .default_height(300.0)
            .open(&mut window_open)
            .show(ctx, |ui| {
                ui.label("📊 Performance Metrics");
                ui.horizontal(|ui| {
                    ui.label("FPS:");
                    ui.label(
                        egui::RichText::new(format!(
                            "{:.1}",
                            self.state.performance_metrics.average_fps()
                        ))
                        .color(colors::GREEN),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Frame Time:");
                    ui.label(format!(
                        "{:.2} ms",
                        self.state.performance_metrics.average_frame_time_ms()
                    ));
                });
                ui.horizontal(|ui| {
                    ui.label("Render Time:");
                    ui.label(format!(
                        "{:.2} ms",
                        self.state.performance_metrics.average_render_time_ms()
                    ));
                });

                ui.separator();

                ui.label("💻 System");
                if let Some(snapshot) = self.state.system_monitor.get_snapshot() {
                    ui.horizontal(|ui| {
                        ui.label("CPU:");
                        let cpu_color = if snapshot.cpu_average > 80.0 {
                            colors::ERROR
                        } else if snapshot.cpu_average > 60.0 {
                            colors::WARNING
                        } else {
                            colors::SUCCESS
                        };
                        ui.label(
                            egui::RichText::new(format!("{:.1}%", snapshot.cpu_average))
                                .color(cpu_color),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Memory:");
                        let mem_color = if snapshot.memory_usage_percent > 80.0 {
                            colors::ERROR
                        } else if snapshot.memory_usage_percent > 60.0 {
                            colors::WARNING
                        } else {
                            colors::SUCCESS
                        };
                        ui.label(
                            egui::RichText::new(format!("{:.1}%", snapshot.memory_usage_percent))
                                .color(mem_color),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Uptime:");
                        ui.label(self.state.system_monitor.get_uptime_string());
                    });
                }

                ui.separator();

                ui.label("🔊 Audio System");
                let stats = self.state.audio_manager.get_stats();
                ui.horizontal(|ui| {
                    ui.label("Device Count:");
                    ui.label(format!("{}", stats.total_devices));
                });
                ui.horizontal(|ui| {
                    ui.label("Devices w/ volume:");
                    ui.label(format!("{}", stats.devices_with_volume));
                });
                ui.horizontal(|ui| {
                    ui.label("Muted Devices:");
                    ui.label(format!("{}", stats.muted_devices));
                });

                ui.separator();

                ui.horizontal(|ui| {
                    if ui.small_button("🔄 Refresh Audio").clicked() {
                        if let Err(e) = self.state.audio_manager.refresh_devices() {
                            error!("Failed to refresh audio devices: {}", e);
                        }
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("❌ Close").clicked() {
                            self.state.ui_state.toggle_debug_window();
                        }
                    });
                });
            });

        if !window_open || ctx.input(|i| i.viewport().close_requested()) {
            self.state.ui_state.toggle_debug_window();
        }
    }
}

impl eframe::App for EguiBarApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.set_pixels_per_point(self.state.ui_state.scale_factor);

        self.state.update();

        #[cfg(feature = "debug_mode")]
        {
            let mut setting = true;
            egui::Window::new("🔧 Settings")
                .open(&mut setting)
                .vscroll(true)
                .show(ctx, |ui| {
                    ctx.settings_ui(ui);
                });

            egui::Window::new("🔍 Inspection")
                .open(&mut setting)
                .vscroll(true)
                .show(ctx, |ui| {
                    ctx.inspection_ui(ui);
                });
        }

        egui::CentralPanel::default()
            .frame(
                egui::Frame::default()
                    .fill(Color32::WHITE)
                    .inner_margin(egui::Margin::symmetric(8, 4)),
            )
            .show(ctx, |ui| {
                self.draw_main_ui(ui);
                self.draw_volume_control_window(ctx);
                self.draw_debug_display_window(ctx);
                self.adjust_window(ctx, ui);
            });

        if self.state.ui_state.need_resize {
            info!("request for resize");
            ctx.request_repaint_after(std::time::Duration::from_millis(1));
        }
    }
}
