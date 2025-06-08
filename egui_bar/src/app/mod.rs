//! Application core module

pub mod events;
pub mod state;

use crate::config::AppConfig;
use crate::constants::{colors, icons, ui};
use crate::ui::components::{SystemInfoPanel, VolumeControlWindow, WorkspacePanel};
use crate::utils::{AppError, PerformanceMetrics, Result};
use eframe::egui;
use egui::{Align, Color32, FontFamily, FontId, Layout, Margin, TextStyle};
use events::{AppEvent, EventBus};
use log::{debug, error, info, warn};
use shared_structures::SharedMessage;
use std::collections::BTreeMap;
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub use state::{AppState, UiState, VolumeWindowState};

/// Main egui application
pub struct EguiBarApp {
    /// Application state
    state: AppState,

    /// Event bus
    event_bus: EventBus,

    /// Message receiver from background thread
    message_receiver: mpsc::Receiver<SharedMessage>,

    /// Resize request sender
    resize_sender: mpsc::Sender<bool>,

    /// UI components
    volume_window: VolumeControlWindow,
    system_info_panel: SystemInfoPanel,
    workspace_panel: WorkspacePanel,

    /// Initialization flag
    initialized: bool,
}

impl EguiBarApp {
    /// Create new application instance
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        message_receiver: mpsc::Receiver<SharedMessage>,
        resize_sender: mpsc::Sender<bool>,
    ) -> Result<Self> {
        // Load configuration
        let mut config = AppConfig::load()?;
        config.validate()?;

        // Initialize application state
        let state = AppState::new(config);

        // Initialize event bus
        let event_bus = EventBus::new();

        // Setup fonts
        Self::setup_fonts(&cc.egui_ctx)?;

        // Apply theme
        state.theme_manager.apply_to_context(&cc.egui_ctx);

        // Configure text styles
        Self::configure_text_styles(&cc.egui_ctx, state.ui_state.scale_factor);

        Ok(Self {
            state,
            event_bus,
            message_receiver,
            resize_sender,
            volume_window: VolumeControlWindow::new(),
            system_info_panel: SystemInfoPanel::new(),
            workspace_panel: WorkspacePanel::new(),
            initialized: false,
        })
    }

    /// Setup system fonts (简化版本)
    fn setup_fonts(ctx: &egui::Context) -> Result<()> {
        use font_kit::family_name::FamilyName;
        use font_kit::properties::Properties;
        use font_kit::source::SystemSource;

        info!("Loading system fonts...");
        let mut fonts = egui::FontDefinitions::default();
        let system_source = SystemSource::new();

        for &font_name in crate::constants::FONT_FAMILIES {
            match system_source.select_best_match(
                &[FamilyName::Title(font_name.to_string())],
                &Properties::new(),
            ) {
                Ok(font_handle) => {
                    match font_handle.load() {
                        Ok(font) => {
                            if let Some(font_data) = font.copy_font_data() {
                                // 使用 .into() 自动转换为 Arc<FontData>
                                fonts.font_data.insert(
                                    font_name.to_string(),
                                    egui::FontData::from_owned(font_data.to_vec()).into(),
                                );

                                fonts
                                    .families
                                    .get_mut(&FontFamily::Monospace)
                                    .unwrap()
                                    .insert(0, font_name.to_string());

                                info!("Loaded font: {}", font_name);
                                break; // Use first available font
                            }
                        }
                        Err(e) => debug!("Failed to load font {}: {}", font_name, e),
                    }
                }
                Err(e) => debug!("Font {} not found: {}", font_name, e),
            }
        }

        ctx.set_fonts(fonts);
        Ok(())
    }

    /// Configure text styles
    pub fn configure_text_styles(ctx: &egui::Context, scale_factor: f32) {
        ctx.all_styles_mut(|style| {
            let base_font_size = ui::DEFAULT_FONT_SIZE / scale_factor;

            let text_styles: BTreeMap<TextStyle, FontId> = [
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
                    TextStyle::Small,
                    FontId::new(base_font_size * 0.8, FontFamily::Proportional),
                ),
                (
                    TextStyle::Heading,
                    FontId::new(base_font_size * 1.5, FontFamily::Proportional),
                ),
            ]
            .into();

            style.text_styles = text_styles;
            style.spacing.window_margin = Margin::same(0);
            style.spacing.menu_margin = Margin::same(0);
        });
    }

    /// Handle incoming messages
    fn handle_messages(&mut self) {
        while let Ok(message) = self.message_receiver.try_recv() {
            self.state.current_message = Some(message);
            self.state.ui_state.need_resize = true;
        }
    }

    /// Handle application events
    fn handle_events(&mut self) {
        let event_sender = self.event_bus.sender();

        self.event_bus.process_events(|event| match event {
            AppEvent::VolumeAdjust { device_name, delta } => {
                if let Err(e) = self.state.audio_manager.adjust_volume(&device_name, delta) {
                    error!("Failed to adjust volume: {}", e);
                }
            }

            AppEvent::ToggleMute(device_name) => {
                if let Err(e) = self.state.audio_manager.toggle_mute(&device_name) {
                    error!("Failed to toggle mute: {}", e);
                }
            }

            AppEvent::WindowResize {
                width: _,
                height: _,
            } => {
                self.state.ui_state.request_resize();
            }

            AppEvent::ScaleFactorChanged(factor) => {
                self.state.ui_state.scale_factor = factor;
                self.state.ui_state.request_resize();
            }

            AppEvent::ThemeChanged(theme_name) => {
                if let Ok(theme_type) = theme_name.parse() {
                    self.state.theme_manager.set_theme(theme_type);
                }
            }

            AppEvent::TimeFormatToggle => {
                self.state.ui_state.toggle_time_format();
            }

            AppEvent::ScreenshotRequested => {
                let _ = Command::new("flameshot").arg("gui").spawn();
            }

            AppEvent::SettingsToggle => {
                self.state.ui_state.show_settings_window =
                    !self.state.ui_state.show_settings_window;
            }

            AppEvent::DebugToggle => {
                self.state.ui_state.show_debug_window = !self.state.ui_state.show_debug_window;
            }

            AppEvent::SaveConfig => {
                if let Err(e) = self.state.save_config() {
                    error!("Failed to save configuration: {}", e);
                }
            }

            _ => {
                debug!("Unhandled event: {:?}", event);
            }
        });
    }

    /// Calculate window dimensions
    fn calculate_window_dimensions(&self) -> (f32, f32, egui::Pos2) {
        if let Some(ref message) = self.state.current_message {
            let monitor_info = &message.monitor_info;
            let base_height = if self.state.ui_state.volume_window.open {
                monitor_info.monitor_height as f32 * 0.3
            } else {
                monitor_info.monitor_height as f32 * 0.03
            };

            let width = (monitor_info.monitor_width as f32 - 2.0 * monitor_info.border_w as f32)
                / self.state.ui_state.scale_factor;
            let height = base_height / self.state.ui_state.scale_factor;

            let pos = egui::Pos2::new(
                (monitor_info.monitor_x as f32 + monitor_info.border_w as f32)
                    / self.state.ui_state.scale_factor,
                (monitor_info.monitor_y as f32 + monitor_info.border_w as f32 * 0.5)
                    / self.state.ui_state.scale_factor,
            );

            (width, height, pos)
        } else {
            (800.0, ui::DEFAULT_FONT_SIZE * 2.0, egui::Pos2::ZERO)
        }
    }

    /// Adjust window size and position
    fn adjust_window(&mut self, ctx: &egui::Context) {
        if self.state.ui_state.need_resize {
            let (width, height, pos) = self.calculate_window_dimensions();

            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(
                width, height,
            )));

            self.state.ui_state.current_window_height = height;
            self.state.ui_state.need_resize = false;

            debug!("Window adjusted: {}x{} at {:?}", width, height, pos);
        }
    }

    /// Draw main UI
    fn draw_main_ui(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                // Left: Workspace information
                self.workspace_panel
                    .draw(ui, &self.state, &self.event_bus.sender());

                // Center: System information
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // Right side controls
                    self.draw_controls(ui);

                    // System info
                    self.system_info_panel.draw(ui, &self.state);
                });
            });
        });
    }

    /// Draw control buttons (time, volume, etc.)
    fn draw_controls(&mut self, ui: &mut egui::Ui) {
        // Volume button
        self.draw_volume_button(ui);

        // Time display
        self.draw_time_display(ui);

        // Screenshot button
        if ui
            .small_button(format!(
                "{} {:.2}",
                icons::SCREENSHOT_ICON,
                self.state.ui_state.scale_factor
            ))
            .clicked()
        {
            self.event_bus.send(AppEvent::ScreenshotRequested).ok();
        }

        // Monitor number
        if let Some(ref message) = self.state.current_message {
            let monitor_num = (message.monitor_info.monitor_num as usize).min(1);
            ui.label(
                egui::RichText::new(format!("[{}]", icons::MONITOR_NUMBERS[monitor_num])).strong(),
            );
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
                "{}：{}%{}",
                device.description,
                device.volume,
                if device.is_muted { " (已静音)" } else { "" }
            );

            (icon, tooltip)
        } else {
            (icons::VOLUME_MUTED, "无音频设备".to_string())
        };

        let response = ui.button(volume_icon);

        if response.clicked() {
            self.state.ui_state.toggle_volume_window();
        }

        response.on_hover_text(tooltip);
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
            .selectable_label(true, egui::RichText::new(current_time).color(colors::GREEN))
            .clicked()
        {
            self.event_bus.send(AppEvent::TimeFormatToggle).ok();
        }
    }

    /// Draw debug window
    fn draw_debug_window(&mut self, ctx: &egui::Context) {
        if self.state.ui_state.show_debug_window {
            egui::Window::new("Debug Information")
                .resizable(true)
                .show(ctx, |ui| {
                    ui.heading("Performance");
                    ui.label(format!(
                        "FPS: {:.1}",
                        self.state.performance_metrics.average_fps()
                    ));
                    ui.label(format!(
                        "Frame Time: {:.2}ms",
                        self.state.performance_metrics.average_frame_time_ms()
                    ));
                    ui.label(format!(
                        "Render Time: {:.2}ms",
                        self.state.performance_metrics.average_render_time_ms()
                    ));

                    ui.separator();

                    ui.heading("System");
                    if let Some(snapshot) = self.state.system_monitor.get_snapshot() {
                        ui.label(format!("CPU: {:.1}%", snapshot.cpu_average));
                        ui.label(format!("Memory: {:.1}%", snapshot.memory_usage_percent));
                        ui.label(format!(
                            "Uptime: {}",
                            self.state.system_monitor.get_uptime_string()
                        ));
                    }

                    ui.separator();

                    ui.heading("Audio");
                    let stats = self.state.audio_manager.get_stats();
                    ui.label(format!("Devices: {}", stats.total_devices));
                    ui.label(format!(
                        "With Volume Control: {}",
                        stats.devices_with_volume
                    ));
                    ui.label(format!("Muted: {}", stats.muted_devices));

                    if ui.button("Close").clicked() {
                        self.state.ui_state.show_debug_window = false;
                    }
                });
        }
    }
}

impl eframe::App for EguiBarApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Initialize on first frame
        if !self.initialized {
            self.state.theme_manager.apply_to_context(ctx);
            self.initialized = true;
        }

        // Handle scale factor changes
        let current_scale = ctx.pixels_per_point();
        if (current_scale - self.state.ui_state.scale_factor).abs() > 0.01 {
            self.state.ui_state.scale_factor =
                current_scale.clamp(ui::MIN_SCALE_FACTOR, ui::MAX_SCALE_FACTOR);
            Self::configure_text_styles(ctx, self.state.ui_state.scale_factor);
            self.state.ui_state.request_resize();
        }

        // Handle messages and events
        self.handle_messages();
        self.handle_events();

        // Update application state
        self.state.update();

        // Adjust window if needed
        self.adjust_window(ctx);

        // Draw main UI
        self.draw_main_ui(ctx);

        // Draw volume control window
        let volume_closed = self
            .volume_window
            .draw(ctx, &mut self.state, &self.event_bus.sender());
        if volume_closed {
            self.state.ui_state.volume_window.open = false;
            self.state.ui_state.request_resize();
        }

        // Draw debug window
        self.draw_debug_window(ctx);

        // Request repaint if needed
        if self.state.ui_state.need_resize {
            ctx.request_repaint_after(Duration::from_millis(1));
        }
    }
}
