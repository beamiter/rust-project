//! Application core module

pub mod events;
pub mod state;

use crate::constants::ui;
use crate::ui::components::controller_info::ControllerInfoPanel;
use crate::ui::components::{
    DebugDisplayWindow, SystemInfoPanel, VolumeControlWindow, WorkspacePanel,
};
use crate::utils::Result;
use eframe::egui;
use egui::{Align, FontFamily, FontId, Layout, Margin, TextStyle, Vec2};
use events::{AppEvent, EventBus};
use log::{debug, error, info, warn};
use shared_structures::{SharedCommand, SharedMessage};
use state::AppState;
pub use state::{UiState, VolumeWindowState};
use std::collections::BTreeMap;
use std::process::Command;
use std::sync::{mpsc, Arc, Mutex, Once};
use std::thread;
use std::time::{Duration, Instant};

static START: Once = Once::new();

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
pub struct EguiBarApp {
    /// Application state
    state: AppState,

    /// 线程间共享状态
    shared_state: Arc<Mutex<SharedAppState>>,

    /// Event bus
    event_bus: EventBus,

    /// UI components
    volume_window: VolumeControlWindow,
    debug_window: DebugDisplayWindow,

    system_info_panel: SystemInfoPanel,
    controller_info_panel: ControllerInfoPanel,
    workspace_panel: WorkspacePanel,

    command_sender: mpsc::Sender<SharedCommand>,
}

impl EguiBarApp {
    /// Create new application instance
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        message_receiver: mpsc::Receiver<SharedMessage>,
        command_sender: mpsc::Sender<SharedCommand>,
    ) -> Result<Self> {
        // Initialize application state
        let state = AppState::new();

        // Initialize event bus
        let event_bus = EventBus::new();

        // 创建共享状态
        let shared_state = Arc::new(Mutex::new(SharedAppState::new()));

        // Setup fonts
        Self::setup_custom_fonts(&cc.egui_ctx)?;

        // Apply theme
        state.theme_manager.apply_to_context(&cc.egui_ctx);

        // Configure text styles
        Self::configure_text_styles(&cc.egui_ctx);

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
            debug_window: DebugDisplayWindow::new(),
            system_info_panel: SystemInfoPanel::new(),
            controller_info_panel: ControllerInfoPanel::new(),
            workspace_panel: WorkspacePanel::new(),
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
            thread::sleep(Duration::from_millis(500));
            let current_second = chrono::Local::now().timestamp();
            if current_second != last_second {
                last_second = current_second;
                egui_ctx.request_repaint_after(Duration::from_millis(1));
            }
        }
    }

    fn setup_custom_fonts(ctx: &egui::Context) -> Result<()> {
        use font_kit::family_name::FamilyName;
        use font_kit::properties::Properties;
        use font_kit::source::SystemSource;
        use std::collections::HashSet;

        info!("Loading system fonts...");
        let mut fonts = egui::FontDefinitions::default();
        let system_source = SystemSource::new();

        // 保存原始字体族
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
        let mut seen_fonts = HashSet::new(); // 避免重复加载相同字体
        for &font_name in crate::constants::FONT_FAMILIES {
            // 跳过已经存在的字体
            if fonts.font_data.contains_key(font_name) || seen_fonts.contains(font_name) {
                info!("Font {} already loaded, skipping", font_name);
                continue;
            }
            info!("Attempting to load font: {}", font_name);
            // 分步处理，避免错误类型不匹配
            let font_result = system_source
                .select_best_match(
                    &[FamilyName::Title(font_name.to_string())],
                    &Properties::new(),
                )
                .and_then(|handle| {
                    // 将 FontLoadingError 转换为 SelectionError
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

        // 只有成功加载字体时才更新字体族配置
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
            ); // 减去egui默认的2个字体
        } else {
            info!("No custom fonts loaded, using default configuration");
        }

        ctx.set_fonts(fonts);
        Ok(())
    }

    fn update_font_families(
        fonts: &mut egui::FontDefinitions,
        loaded_fonts: Vec<String>,
        original_proportional: Vec<String>,
        original_monospace: Vec<String>,
    ) {
        // 构建新的字体族列表：自定义字体 + 原始字体
        let new_proportional = [loaded_fonts.clone(), original_proportional].concat();
        let new_monospace = [loaded_fonts.clone(), original_monospace].concat();

        fonts
            .families
            .insert(FontFamily::Proportional, new_proportional);
        fonts.families.insert(FontFamily::Monospace, new_monospace);

        // 调试信息
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
                (
                    TextStyle::Small,
                    FontId::new(base_font_size * 0.8, FontFamily::Proportional),
                ),
                (
                    TextStyle::Body,
                    FontId::new(base_font_size, FontFamily::Proportional),
                ),
                (
                    TextStyle::Monospace,
                    FontId::new(base_font_size, FontFamily::Proportional),
                ),
                (
                    TextStyle::Button,
                    FontId::new(base_font_size, FontFamily::Proportional),
                ),
                (
                    TextStyle::Heading,
                    FontId::new(base_font_size * 1.5, FontFamily::Proportional),
                ),
            ]
            .into();
            style.text_styles = text_styles;
            style.spacing.window_margin = Margin::ZERO;
            style.spacing.menu_margin = Margin::ZERO;
            style.spacing.menu_spacing = 0.;
            style.spacing.button_padding = Vec2::new(2., 1.);
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

            _ => {
                debug!("Unhandled event: {:?}", event);
            }
        });
    }

    /// Calculate window dimensions
    fn calculate_window_dimensions(&self, _ui: &egui::Ui) -> Option<(f32, f32, egui::Pos2)> {
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
                40.
            };

            let width = monitor_info.monitor_width as f32 - 2.0 * monitor_info.border_w as f32;
            let button_height = self.state.ui_state.button_height + 8. * 2.;
            info!("button_height: {button_height}");
            let height = base_height.max(button_height);

            let pos = egui::Pos2::new(
                monitor_info.monitor_x as f32 + monitor_info.border_w as f32,
                monitor_info.monitor_y as f32 + monitor_info.border_w as f32 * 0.5,
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
                let viewport_info = ctx.input(|i| i.viewport().clone());
                info!("screen_rect: {:?}", viewport_info);
                let outer_rect = viewport_info.outer_rect.unwrap();
                if (outer_rect.width() - width).abs() > 5.
                    || (outer_rect.height() - height).abs() > 5.
                {
                    info!("Window adjusted: {}x{} at {:?}", width, height, pos);
                } else {
                    self.state.ui_state.need_resize = false;
                }
            }
        }
    }

    /// Draw main UI
    fn draw_main_ui(&mut self, ui: &mut egui::Ui, event_sender: &mpsc::Sender<AppEvent>) {
        // 更新当前消息到状态中
        if let Some(message) = self.get_current_message() {
            self.state.current_message = Some(message);
        }

        ui.horizontal_centered(|ui| {
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                self.workspace_panel
                    .draw(ui, &mut self.state, &self.command_sender);
            });

            ui.columns(2, |ui| {
                ui[0].with_layout(Layout::left_to_right(Align::Center), |_ui| {});

                ui[1].with_layout(Layout::right_to_left(Align::Center), |ui| {
                    self.controller_info_panel
                        .draw(ui, &mut self.state, event_sender);
                    // ui.separator();
                    self.system_info_panel.draw(ui, &self.state);
                });
            });
        });
    }
}

impl eframe::App for EguiBarApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        START.call_once(|| {
            self.state.ui_state.need_resize = true;
        });
        ctx.set_pixels_per_point(self.state.ui_state.scale_factor);

        // Handle events
        self.handle_events();

        // Update application state (system monitoring, audio, etc.)
        self.state.update();

        // Draw main UI
        egui::CentralPanel::default().show(ctx, |ui| {
            self.draw_main_ui(ui, &self.event_bus.sender());

            // Draw volume control window
            self.volume_window
                .draw(ctx, &mut self.state, &self.event_bus.sender());

            // Draw debug display window
            self.debug_window
                .draw(ctx, &mut self.state, &self.event_bus.sender());

            // Adjust window if needed
            self.adjust_window(ctx, ui);
        });

        if self.state.ui_state.need_resize {
            info!("request for resize");
            ctx.request_repaint_after(Duration::from_millis(1));
        }
    }
}
