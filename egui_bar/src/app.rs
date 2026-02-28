use egui::{Align, Button, Label, Layout, Sense, Stroke};
use log::{error, info};
use shared_structures::SharedRingBuffer;
use std::sync::{Arc, Mutex};

use anyhow::Result;

use crate::animation::AnimationState;
use crate::config;
use crate::events::{self, BarEvent, EventBus};
use crate::ipc;
use crate::modules::ModuleRegistry;
use crate::state::{AppState, SharedAppState};
use crate::theme::{self, colors, icons};

/// Main egui application
pub struct EguiBarApp {
    /// Application state
    state: AppState,
    /// Thread-safe shared state
    shared_state: Arc<Mutex<SharedAppState>>,
    /// Shared buffer for communication (retained for future use by non-module code)
    #[allow(dead_code)]
    shared_buffer_rc: Option<Arc<SharedRingBuffer>>,
    /// Module registry
    modules: ModuleRegistry,
    /// Animation state
    anim: AnimationState,
    /// Event bus
    event_bus: EventBus,
}

impl EguiBarApp {
    /// Create new application instance
    pub fn new(cc: &eframe::CreationContext<'_>, shared_path: String, rt_handle: tokio::runtime::Handle) -> Result<Self> {
        theme::apply_theme(&cc.egui_ctx);
        let state = AppState::new();
        let shared_state = Arc::new(Mutex::new(SharedAppState::new()));

        #[cfg(feature = "debug_mode")]
        {
            cc.egui_ctx.set_debug_on_hover(true);
        }

        theme::setup_custom_fonts(&cc.egui_ctx)?;
        theme::configure_text_styles(&cc.egui_ctx);

        let shared_buffer_rc =
            SharedRingBuffer::create_shared_ring_buffer_aux(&shared_path).map(Arc::new);

        ipc::start_background_tasks(&shared_state, &cc.egui_ctx, shared_buffer_rc.clone());
        config::start_config_watcher(cc.egui_ctx.clone());

        let modules = ModuleRegistry::from_config(&shared_buffer_rc, &cc.egui_ctx, &rt_handle);
        let anim = AnimationState::new();

        // Start event-driven listeners
        let event_bus = EventBus::new();
        events::start_event_listeners(event_bus.sender(), cc.egui_ctx.clone());

        Ok(Self {
            state,
            shared_state,
            shared_buffer_rc,
            modules,
            anim,
            event_bus,
        })
    }

    /// Get current message from shared state
    fn get_current_message(&self) -> Option<shared_structures::SharedMessage> {
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
        let cfg = config::CONFIG.load();
        cfg.general.height
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

    /// Dispatch an event to the appropriate handler
    fn dispatch_event(&mut self, event: BarEvent) {
        match event {
            BarEvent::BatteryChanged { .. } => {
                // Battery module polls SystemMonitor, which will pick this up
                log::debug!("Battery event received");
            }
            BarEvent::NetworkChanged { interface, connected } => {
                log::debug!("Network event: {} {}", interface, if connected { "up" } else { "down" });
            }
            BarEvent::BrightnessChanged { value, max } => {
                log::debug!("Brightness event: {}/{}", value, max);
            }
            BarEvent::BluetoothDeviceChanged { address, connected } => {
                log::debug!("Bluetooth event: {} {}", address, if connected { "connected" } else { "disconnected" });
            }
            BarEvent::MediaPlayerChanged { player, title, .. } => {
                log::debug!("Media event: {} - {}", player, title);
            }
            BarEvent::MediaPlaybackChanged { status } => {
                log::debug!("Media playback: {}", status);
            }
            BarEvent::WorkspaceChanged { message } => {
                self.state.current_message = Some(message);
            }
            BarEvent::ConfigReloaded => {
                log::info!("Config reloaded event received");
            }
            BarEvent::TrayItemAdded { id } => {
                log::debug!("Tray item added: {}", id);
            }
            BarEvent::TrayItemRemoved { id } => {
                log::debug!("Tray item removed: {}", id);
            }
        }
    }

    // ================================
    // Main UI via Module System
    // ================================

    fn draw_main_ui(&mut self, ui: &mut egui::Ui) {
        // Sync shared state
        if let Some(message) = self.get_current_message() {
            self.state.current_message = Some(message);
        }

        ui.horizontal_centered(|ui| {
            // Left modules
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                for module in &mut self.modules.left {
                    module.render_bar(ui, &mut self.state, &mut self.anim);
                }
            });

            ui.columns(2, |ui| {
                // Center modules
                ui[0].with_layout(Layout::left_to_right(Align::Center), |ui| {
                    for module in &mut self.modules.center {
                        module.render_bar(ui, &mut self.state, &mut self.anim);
                    }
                });

                // Right modules (RTL)
                ui[1].with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // Extra buttons not in module system (debug, screenshot, monitor)
                    self.draw_extra_buttons(ui);

                    // Right modules in reverse order (RTL layout)
                    for module in self.modules.right.iter_mut().rev() {
                        module.render_bar(ui, &mut self.state, &mut self.anim);
                    }
                });
            });
        });
    }

    /// Extra buttons that remain in app.rs (screenshot, debug, monitor)
    fn draw_extra_buttons(&mut self, ui: &mut egui::Ui) {
        // Debug button
        {
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

        // Screenshot button
        {
            let label_response = ui.add(Button::new(icons::SCREENSHOT_ICON));
            if label_response.clicked() {
                let _ = std::process::Command::new("flameshot").arg("gui").spawn();
            }
            label_response.on_hover_text(format!(
                "Screenshot (flameshot)\nScale: {:.2}",
                self.state.ui_state.scale_factor
            ));
        }

        // Monitor number
        if let Some(ref message) = self.state.current_message {
            let monitor_num = (message.monitor_info.monitor_num as usize).min(1);
            ui.add(Label::new(
                egui::RichText::new(format!("{}", icons::MONITOR_NUMBERS[monitor_num])).strong(),
            ));
        }
    }

    /// Render all module popups
    fn render_popups(&mut self, ctx: &egui::Context) {
        // Render module popups
        for module in self.modules.right.iter_mut()
            .chain(self.modules.left.iter_mut())
            .chain(self.modules.center.iter_mut())
        {
            if module.has_popup() {
                module.render_popup(ctx, &mut self.state);
            }
        }

        // Debug window (not a module)
        self.draw_debug_display_window(ctx);
    }

    // ================================
    // Debug Window
    // ================================

    fn draw_debug_display_window(&mut self, ctx: &egui::Context) {
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

        // Process events from the event bus
        for event in self.event_bus.drain() {
            self.dispatch_event(event);
        }

        self.state.update();

        // Update all modules
        for module in self.modules.left.iter_mut()
            .chain(self.modules.center.iter_mut())
            .chain(self.modules.right.iter_mut())
        {
            module.update(&self.state);
        }

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
                    .fill(colors::BG)
                    .stroke(Stroke::new(1.0, colors::STROKE_SUBTLE))
                    .inner_margin(egui::Margin::symmetric(10, 2)),
            )
            .show(ctx, |ui| {
                self.draw_main_ui(ui);
                self.render_popups(ctx);
                self.adjust_window(ctx, ui);
            });

        // Request repaint if animations are running
        if self.anim.is_animating() {
            ctx.request_repaint();
        }

        // Periodic GC for animation entries
        self.anim.gc();

        if self.state.ui_state.need_resize {
            info!("request for resize");
            ctx.request_repaint_after(std::time::Duration::from_millis(1));
        }
    }
}
