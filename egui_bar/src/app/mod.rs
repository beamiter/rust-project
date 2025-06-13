//! Application core module

pub mod events;
pub mod state;

use crate::config::AppConfig;
use crate::constants::{colors, icons, ui};
use crate::ui::components::{SystemInfoPanel, VolumeControlWindow, WorkspacePanel};
use crate::utils::Result;
use eframe::egui;
use egui::{Align, FontFamily, FontId, Layout, Margin, TextStyle};
use events::{AppEvent, EventBus};
use log::{debug, error, info, warn};
use shared_structures::{SharedCommand, SharedMessage};
use state::AppState;
use std::collections::BTreeMap;
use std::process::Command;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub use state::{UiState, VolumeWindowState};

/// 线程间共享的应用状态
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

/// Main egui application
#[allow(dead_code)]
pub struct EguiBarApp {
    /// Application state
    state: AppState,

    /// 线程间共享状态
    shared_state: Arc<Mutex<SharedAppState>>,

    /// Event bus
    event_bus: EventBus,

    /// UI components
    volume_window: VolumeControlWindow,
    system_info_panel: SystemInfoPanel,
    workspace_panel: WorkspacePanel,

    /// Initialization flag
    initialized: bool,

    /// egui context for requesting repaints
    egui_ctx: egui::Context,

    command_sender: mpsc::Sender<SharedCommand>,
}

impl EguiBarApp {
    /// Create new application instance
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        message_receiver: mpsc::Receiver<SharedMessage>,
        command_sender: mpsc::Sender<SharedCommand>,
    ) -> Result<Self> {
        // Load configuration
        let mut config = AppConfig::load()?;
        config.validate()?;

        // Initialize application state
        let state = AppState::new(config);

        // Initialize event bus
        let event_bus = EventBus::new();

        // 创建共享状态
        let shared_state = Arc::new(Mutex::new(SharedAppState::new()));

        // Setup fonts
        Self::setup_custom_fonts(&cc.egui_ctx)?;

        // Apply theme
        state.theme_manager.apply_to_context(&cc.egui_ctx);

        // Configure text styles
        Self::configure_text_styles(&cc.egui_ctx, state.ui_state.scale_factor);

        // 启动消息处理线程
        let shared_state_clone = Arc::clone(&shared_state);
        let egui_ctx_clone = cc.egui_ctx.clone();
        thread::spawn(move || {
            Self::message_handler_thread(message_receiver, shared_state_clone, egui_ctx_clone);
        });

        // 启动定时更新线程
        let shared_state_clone = Arc::clone(&shared_state);
        let egui_ctx_clone = cc.egui_ctx.clone();
        thread::spawn(move || {
            Self::periodic_update_thread(shared_state_clone, egui_ctx_clone);
        });

        Ok(Self {
            state,
            shared_state,
            event_bus,
            volume_window: VolumeControlWindow::new(),
            system_info_panel: SystemInfoPanel::new(),
            workspace_panel: WorkspacePanel::new(),
            initialized: false,
            egui_ctx: cc.egui_ctx.clone(),
            command_sender,
        })
    }

    /// 消息处理线程
    fn message_handler_thread(
        message_receiver: mpsc::Receiver<SharedMessage>,
        shared_state: Arc<Mutex<SharedAppState>>,
        egui_ctx: egui::Context,
    ) {
        info!("Starting message handler thread");

        // 设置 panic 钩子
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            error!("Message handler thread panicked: {}", panic_info);
            default_hook(panic_info);
        }));

        loop {
            match message_receiver.recv() {
                Ok(message) => {
                    info!("Received message: {:?}", message);

                    // 更新共享状态
                    if let Ok(mut state) = shared_state.lock() {
                        let need_update = state
                            .current_message
                            .as_ref()
                            .map(|m| m.timestamp != message.timestamp)
                            .unwrap_or(true);

                        if need_update {
                            state.current_message = Some(message);
                            state.last_update = Instant::now();

                            egui_ctx.request_repaint_after(Duration::from_millis(1));
                        }
                    } else {
                        warn!("Failed to lock shared state for message update");
                    }
                }
                Err(e) => {
                    error!("Message receiver error: {}", e);
                    break;
                }
            }
        }

        info!("Message handler thread exiting");
    }

    /// 定时更新线程（每秒更新时间显示等）
    fn periodic_update_thread(_shared_state: Arc<Mutex<SharedAppState>>, egui_ctx: egui::Context) {
        info!("Starting periodic update thread");

        let mut last_second = chrono::Local::now().timestamp();

        loop {
            thread::sleep(Duration::from_millis(500)); // 每500ms检查一次

            let current_second = chrono::Local::now().timestamp();

            // 每秒更新一次
            if current_second != last_second {
                last_second = current_second;

                egui_ctx.request_repaint_after(Duration::from_millis(1));
            }
        }
    }

    /// Setup system fonts
    fn setup_custom_fonts(ctx: &egui::Context) -> Result<()> {
        use font_kit::family_name::FamilyName;
        use font_kit::properties::Properties;
        use font_kit::source::SystemSource;

        info!("Loading system fonts...");
        let mut fonts = egui::FontDefinitions::default();
        let system_source = SystemSource::new();

        for &font_name in crate::constants::FONT_FAMILIES {
            info!("font_name: {}", font_name);
            match system_source.select_best_match(
                &[FamilyName::Title(font_name.to_string())],
                &Properties::new(),
            ) {
                Ok(font_handle) => {
                    match font_handle.load() {
                        Ok(font) => {
                            if let Some(font_data) = font.copy_font_data() {
                                fonts.font_data.insert(
                                    font_name.to_string(),
                                    egui::FontData::from_owned(font_data.to_vec()).into(),
                                );

                                fonts
                                    .families
                                    .get_mut(&FontFamily::Monospace)
                                    .unwrap()
                                    .insert(0, font_name.to_string());
                                fonts
                                    .families
                                    .entry(egui::FontFamily::Proportional)
                                    .or_default()
                                    .insert(0, font_name.to_string());

                                info!("Loaded font: {}", font_name);
                                // break; // Use first available font
                            }
                        }
                        Err(e) => info!("Failed to load font {}: {}", font_name, e),
                    }
                }
                Err(e) => info!("Font {} not found: {}", font_name, e),
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
                    FontId::new(base_font_size * 0.8, FontFamily::Monospace),
                ),
                (
                    TextStyle::Heading,
                    FontId::new(base_font_size * 1.5, FontFamily::Monospace),
                ),
            ]
            .into();

            style.text_styles = text_styles;
            style.spacing.window_margin = Margin::same(0);
            style.spacing.menu_margin = Margin::same(0);
        });
    }

    /// 从共享状态获取当前消息
    fn get_current_message(&self) -> Option<SharedMessage> {
        self.shared_state
            .lock()
            .ok()
            .and_then(|state| state.current_message.clone())
    }

    /// Handle application events
    fn handle_events(&mut self) {
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
                // 使用新的 toggle_debug_window 方法
                self.state.ui_state.toggle_debug_window();
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

    fn get_default_button_height(ui: &egui::Ui) -> f32 {
        let style = ui.style();
        // 按钮相关的样式属性
        let button_padding = style.spacing.button_padding; // Vec2
        let item_spacing = style.spacing.item_spacing; // Vec2
        let window_margin = style.spacing.window_margin; // Margin
        info!(
            "button_padding: {button_padding}, item_spacing: {item_spacing}, window_margin: {:?}",
            window_margin
        );

        // 字体相关
        let font_id = &style.text_styles[&egui::TextStyle::Button];
        let text_height = ui.fonts(|fonts| fonts.row_height(font_id));

        // 颜色和视觉效果
        // let button_fill = style.visuals.widgets.inactive.bg_fill;
        // let button_stroke = style.visuals.widgets.inactive.bg_stroke;

        let button_padding = style.spacing.button_padding;

        let button_height = text_height + button_padding.x * 4.0;

        button_height
    }

    /// Calculate window dimensions
    fn calculate_window_dimensions(&self, ui: &egui::Ui) -> Option<(f32, f32, egui::Pos2)> {
        if let Some(message) = self.get_current_message() {
            let monitor_info = &message.monitor_info;

            // 根据打开的窗口数量调整高度
            let base_height = if self.state.ui_state.volume_window.open
                || self.state.ui_state.show_debug_window
            {
                // 如果有任何窗口打开，使用更大的高度
                monitor_info.monitor_height as f32 * 0.618
            } else {
                // 否则使用默认紧凑高度
                monitor_info.monitor_height as f32 * 0.03
            };

            let width = (monitor_info.monitor_width as f32 - 2.0 * monitor_info.border_w as f32)
                / self.state.ui_state.scale_factor;
            let button_height = Self::get_default_button_height(ui);
            info!("button_height: {button_height}");
            let height = (base_height / self.state.ui_state.scale_factor).max(button_height);

            let pos = egui::Pos2::new(
                (monitor_info.monitor_x as f32 + monitor_info.border_w as f32)
                    / self.state.ui_state.scale_factor,
                (monitor_info.monitor_y as f32 + monitor_info.border_w as f32 * 0.5)
                    / self.state.ui_state.scale_factor,
            );

            Some((width, height, pos))
        } else {
            None
        }
    }

    /// Adjust window size and position
    fn adjust_window(&mut self, ctx: &egui::Context, ui: &egui::Ui) {
        if self.state.ui_state.need_resize {
            // Try to adjust window unless get window dimensions.
            if let Some((width, height, pos)) = self.calculate_window_dimensions(ui) {
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(
                    width, height,
                )));

                self.state.ui_state.current_window_height = height;
                self.state.ui_state.need_resize = false;
                info!("Window adjusted: {}x{} at {:?}", width, height, pos);
            }
        }
    }

    /// Draw main UI
    fn draw_main_ui(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        // 更新当前消息到状态中
        if let Some(message) = self.get_current_message() {
            self.state.current_message = Some(message);
        }

        ui.horizontal_centered(|ui| {
            // Left: Workspace information
            self.workspace_panel
                .draw(ui, &mut self.state, &self.command_sender);

            // Center: System information
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // Right side controls
                self.draw_controls(ui, ctx);

                // System info
                self.system_info_panel.draw(ui, &self.state);
            });
        });
    }

    fn draw_battery_info(&self, ui: &mut egui::Ui) {
        if let Some(snapshot) = self.state.system_monitor.get_snapshot() {
            // 获取电池电量百分比
            let battery_percent = snapshot.battery_percent;
            let is_charging = snapshot.is_charging;

            // 根据电量选择颜色
            let battery_color = match battery_percent {
                p if p > 50.0 => colors::BATTERY_HIGH,   // 高电量 - 绿色
                p if p > 20.0 => colors::BATTERY_MEDIUM, // 中电量 - 黄色
                _ => colors::BATTERY_LOW,                // 低电量 - 红色
            };

            // 显示电池图标和电量
            let battery_icon = if is_charging {
                "🔌" // 充电图标
            } else {
                match battery_percent {
                    p if p > 75.0 => "🔋", // 满电池
                    p if p > 50.0 => "🔋", // 高电量
                    p if p > 25.0 => "🪫", // 中电量
                    _ => "🪫",             // 低电量
                }
            };

            // 显示电池图标
            ui.label(egui::RichText::new(battery_icon).color(battery_color));

            // 显示电量百分比
            ui.label(egui::RichText::new(format!("{:.0}%", battery_percent)).color(battery_color));

            // 低电量警告
            if battery_percent < self.state.config.system.battery_warning_threshold * 100.0
                && !is_charging
            {
                ui.label(egui::RichText::new("⚠️").color(colors::WARNING));
            }

            // 充电指示
            if is_charging {
                ui.label(egui::RichText::new("⚡").color(colors::CHARGING));
            }
        } else {
            // 无法获取电池信息时显示
            ui.label(egui::RichText::new("❓").color(colors::UNAVAILABLE));
        }
    }

    /// Draw control buttons (time, volume, debug, etc.)
    fn draw_controls(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // Battery info
        self.draw_battery_info(ui);

        // Volume button
        self.draw_volume_button(ui);

        // Debug button
        self.draw_debug_button(ui);

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

        // 在这里绘制调试窗口（如果打开的话）
        self.draw_debug_window(ctx);
    }

    /// Draw debug control button (类似 draw_volume_button 的逻辑)
    fn draw_debug_button(&mut self, ui: &mut egui::Ui) {
        let (debug_icon, tooltip) = if self.state.ui_state.show_debug_window {
            ("🐛", "关闭调试窗口") // 激活状态的图标和提示
        } else {
            ("🔍", "打开调试窗口") // 默认状态的图标和提示
        };

        let response = ui.button(debug_icon);

        if response.clicked() {
            // 使用新的 toggle_debug_window 方法
            self.state.ui_state.toggle_debug_window();
            info!(
                "Debug window toggled: {}",
                self.state.ui_state.show_debug_window
            );
        }

        // 添加详细的悬停提示信息
        let detailed_tooltip = format!(
            "{}\n📊 性能: {:.1} FPS\n🧵 线程: {} 个活跃\n💾 内存: {:.1}%\n🖥️ CPU: {:.1}%",
            tooltip,
            self.state.performance_metrics.average_fps(),
            2, // 消息处理线程 + 定时更新线程
            self.state
                .system_monitor
                .get_snapshot()
                .map(|s| s.memory_usage_percent)
                .unwrap_or(0.0),
            self.state
                .system_monitor
                .get_snapshot()
                .map(|s| s.cpu_average)
                .unwrap_or(0.0)
        );

        response.on_hover_text(detailed_tooltip);
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

    /// Draw debug window (现在作为弹出窗口显示)
    fn draw_debug_window(&mut self, ctx: &egui::Context) {
        if self.state.ui_state.show_debug_window {
            let mut window_open = true; // 用于检测窗口是否被关闭
                                        //
            egui::Window::new("🐛 调试信息")
                .collapsible(false)
                .resizable(true)
                .default_width(400.0)
                .default_height(300.0)
                .open(&mut window_open)
                .show(ctx, |ui| {
                    ui.heading("📊 性能指标");
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
                        ui.label("帧时间:");
                        ui.label(format!(
                            "{:.2}ms",
                            self.state.performance_metrics.average_frame_time_ms()
                        ));
                    });
                    ui.horizontal(|ui| {
                        ui.label("渲染时间:");
                        ui.label(format!(
                            "{:.2}ms",
                            self.state.performance_metrics.average_render_time_ms()
                        ));
                    });

                    ui.separator();

                    ui.heading("💻 系统状态");
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
                            ui.label("内存:");
                            let mem_color = if snapshot.memory_usage_percent > 80.0 {
                                colors::ERROR
                            } else if snapshot.memory_usage_percent > 60.0 {
                                colors::WARNING
                            } else {
                                colors::SUCCESS
                            };
                            ui.label(
                                egui::RichText::new(format!(
                                    "{:.1}%",
                                    snapshot.memory_usage_percent
                                ))
                                .color(mem_color),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label("运行时间:");
                            ui.label(self.state.system_monitor.get_uptime_string());
                        });
                    }

                    ui.separator();

                    ui.heading("🔊 音频系统");
                    let stats = self.state.audio_manager.get_stats();
                    ui.horizontal(|ui| {
                        ui.label("设备数量:");
                        ui.label(format!("{}", stats.total_devices));
                    });
                    ui.horizontal(|ui| {
                        ui.label("可控音量:");
                        ui.label(format!("{}", stats.devices_with_volume));
                    });
                    ui.horizontal(|ui| {
                        ui.label("已静音:");
                        ui.label(format!("{}", stats.muted_devices));
                    });

                    ui.separator();

                    ui.heading("🧵 线程状态");
                    ui.horizontal(|ui| {
                        ui.label("消息处理:");
                        ui.label(egui::RichText::new("运行中").color(colors::SUCCESS));
                    });
                    ui.horizontal(|ui| {
                        ui.label("定时更新:");
                        ui.label(egui::RichText::new("运行中").color(colors::SUCCESS));
                    });
                    if let Ok(state) = self.shared_state.lock() {
                        ui.horizontal(|ui| {
                            ui.label("最后更新:");
                            ui.label(format!("{:?} 前", state.last_update.elapsed()));
                        });
                    }

                    ui.separator();

                    // 操作按钮
                    ui.horizontal(|ui| {
                        if ui.button("💾 保存配置").clicked() {
                            self.event_bus.send(AppEvent::SaveConfig).ok();
                        }

                        if ui.button("🔄 刷新音频").clicked() {
                            if let Err(e) = self.state.audio_manager.refresh_devices() {
                                error!("Failed to refresh audio devices: {}", e);
                            }
                        }

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button("❌ 关闭").clicked() {
                                self.state.ui_state.toggle_debug_window();
                            }
                        });
                    });
                });
            // 检查窗口是否通过 X 按钮被关闭
            if !window_open && self.state.ui_state.show_debug_window {
                self.state.ui_state.toggle_debug_window();
            }
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

        // Handle events
        self.handle_events();

        // Update application state (system monitoring, audio, etc.)
        self.state.update();

        // Draw main UI
        egui::CentralPanel::default().show(ctx, |ui| {
            self.draw_main_ui(ctx, ui);
            // Draw volume control window
            let volume_closed =
                self.volume_window
                    .draw(ctx, &mut self.state, &self.event_bus.sender());
            if volume_closed {
                self.state.ui_state.volume_window.open = false;
                self.state.ui_state.request_resize();
            }

            // Adjust window if needed
            self.adjust_window(ctx, ui);
        });

        if self.state.ui_state.need_resize {
            info!("request for resize");
            ctx.request_repaint_after(Duration::from_millis(3));
        }
    }
}
