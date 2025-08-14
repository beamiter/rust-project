//! Application core module

use crate::constants::ui;
use crate::constants::{colors, icons};
use crate::{AppState, Result};
use eframe::egui;
use egui::Label;
use egui::Sense;
use egui::{Align, Color32, FontFamily, FontId, Layout, Margin, TextStyle, Vec2};
use egui_plot::{Line, Plot, PlotPoints};
use log::{debug, error, info, warn};
use shared_structures::{SharedCommand, SharedMessage, SharedRingBuffer};
use std::collections::BTreeMap;
use std::process::Command;
use std::sync::{Arc, Mutex, Once};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use egui::{Button, Stroke, StrokeKind};
use shared_structures::CommandType;

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

    color_cache: Vec<Color32>,

    shared_buffer_opt: Option<SharedRingBuffer>,
}

impl EguiBarApp {
    /// Create new application instance
    pub fn new(cc: &eframe::CreationContext<'_>, shared_path: String) -> Result<Self> {
        cc.egui_ctx.set_theme(egui::Theme::Light); // Switch to light mode

        // Initialize application state
        let state = AppState::new();

        // 创建共享状态
        let shared_state = Arc::new(Mutex::new(SharedAppState::new()));

        #[cfg(feature = "debug_mode")]
        {
            cc.egui_ctx.set_debug_on_hover(true);
        }

        // Setup fonts
        Self::setup_custom_fonts(&cc.egui_ctx)?;

        // Configure text styles
        Self::configure_text_styles(&cc.egui_ctx);

        // 启动消息处理线程
        let shared_state_clone = Arc::clone(&shared_state);
        let egui_ctx_clone = cc.egui_ctx.clone();
        let shared_path_clone = shared_path.clone();

        // 启动异步任务
        tokio::spawn(async move {
            Self::shared_memory_worker(shared_path_clone, shared_state_clone, egui_ctx_clone).await;
        });

        // 启动定时更新线程
        let egui_ctx_clone = cc.egui_ctx.clone();
        tokio::spawn(async move {
            Self::periodic_update_task(egui_ctx_clone).await;
        });

        let shared_buffer_opt = SharedRingBuffer::create_shared_ring_buffer(shared_path);

        Ok(Self {
            state,
            shared_state,
            color_cache: Vec::new(),
            shared_buffer_opt,
        })
    }

    async fn shared_memory_worker(
        shared_path: String,
        shared_state: Arc<Mutex<SharedAppState>>,
        egui_ctx: egui::Context,
    ) {
        info!("Starting shared memory worker task");

        // 尝试打开或创建共享环形缓冲区
        let shared_buffer_opt = SharedRingBuffer::create_shared_ring_buffer(shared_path);
        let mut prev_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();

        if let Some(ref shared_buffer) = shared_buffer_opt {
            loop {
                match shared_buffer.wait_for_message(Some(std::time::Duration::from_secs(2))) {
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
                                        egui_ctx.request_repaint_after(
                                            std::time::Duration::from_millis(1),
                                        );
                                    }
                                } else {
                                    warn!("Failed to lock shared state for message update");
                                }
                            }
                        }
                    }
                    Ok(false) => debug!("[notifier] Wait for message timed out."),
                    Err(e) => {
                        error!("[notifier] Wait for message failed: {}", e);
                        break;
                    }
                }
            }
        }

        info!("Shared memory worker task exiting");
    }

    /// 定时更新线程（每秒更新时间显示等）
    async fn periodic_update_task(egui_ctx: egui::Context) {
        info!("Starting periodic update task");
        let mut last_second = chrono::Local::now().timestamp();
        // 创建定时器，每500ms执行一次
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(500));
        loop {
            // 异步等待下一个定时器周期
            interval.tick().await;
            let current_second = chrono::Local::now().timestamp();
            if current_second != last_second {
                last_second = current_second;
                egui_ctx.request_repaint_after(std::time::Duration::from_millis(1));
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
            let target_height = self.state.ui_state.button_height + 3. * 2.;
            info!("target_height: {target_height}");
            let height = base_height.max(target_height);

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
    fn draw_main_ui(&mut self, ui: &mut egui::Ui) {
        // 更新当前消息到状态中
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

    /// Draw volume control window, returns true if window was closed
    pub fn draw_volume_control_window(&mut self, ctx: &egui::Context) {
        if !self.state.ui_state.volume_window.open {
            return;
        }

        let mut window_open = true;

        egui::Window::new("🔊 音量控制")
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
                // Save window position
                if let Some(rect) = ctx.memory(|mem| mem.area_rect(ui.id())) {
                    self.state.ui_state.volume_window.position = Some(rect.left_top());
                }

                self.draw_content(ui);

                // Close button
                ui.horizontal(|ui| {
                    if ui.button("🔧 高级混音器").clicked() {
                        let _ = std::process::Command::new("terminator")
                            .args(["-e", "alsamixer"])
                            .spawn();
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("✖ 关闭").clicked() {
                            self.state.ui_state.toggle_volume_window();
                        }
                    });
                });
            });

        if !window_open || ctx.input(|i| i.viewport().close_requested()) {
            self.state.ui_state.toggle_volume_window();
        }
    }

    fn draw_content(&mut self, ui: &mut egui::Ui) {
        // 先获取设备信息，避免后续的借用冲突
        let devices: Vec<crate::audio_manager::AudioDevice> =
            self.state.audio_manager.get_devices().to_vec();

        if devices.is_empty() {
            ui.add(Label::new("❌ 没有找到可控制的音频设备"));
            return;
        }

        // Filter controllable devices - 现在使用 owned 数据
        let controllable_devices: Vec<(usize, crate::audio_manager::AudioDevice)> = devices
            .into_iter()
            .enumerate()
            .filter(|(_, d)| d.has_volume_control || d.has_switch_control)
            .collect();

        if controllable_devices.is_empty() {
            ui.add(Label::new("❌ 没有找到可控制的音频设备"));
            return;
        }

        // Device selection
        self.draw_device_selector(ui, &controllable_devices);

        ui.add_space(10.0);

        // Device controls - 现在使用 owned 数据
        if let Some((_, device)) =
            controllable_devices.get(self.state.ui_state.volume_window.selected_device)
        {
            self.draw_device_controls(ui, device);
        }
    }

    fn draw_device_selector(
        &mut self,
        ui: &mut egui::Ui,
        controllable_devices: &[(usize, crate::audio_manager::AudioDevice)],
    ) {
        ui.horizontal(|ui| {
            ui.add(Label::new("🎵 设备："));

            // Ensure selected device index is valid
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

    fn draw_device_controls(
        &mut self,
        ui: &mut egui::Ui,
        device: &crate::audio_manager::AudioDevice,
    ) {
        let device_name = device.name.clone();
        let mut current_volume = device.volume;
        let is_muted = device.is_muted;

        // Volume control
        if device.has_volume_control {
            ui.horizontal(|ui| {
                ui.add(Label::new("🔊 音量："));

                // Mute button
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

                    mute_btn.on_hover_text(if is_muted { "取消静音" } else { "静音" });
                }

                // Volume percentage
                ui.label(format!("{}%", current_volume));
            });

            // Volume slider
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
            // Switch-only device
            ui.horizontal(|ui| {
                let btn_text = if is_muted {
                    "🔴 已禁用"
                } else {
                    "🟢 已启用"
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
            ui.add(Label::new("❌ 此设备没有可用的控制选项"));
        }

        // Device info
        ui.separator();
        ui.horizontal(|ui| {
            ui.add(Label::new(format!("📋 类型: {:?}", device.device_type)));
            ui.add(Label::new(format!(
                "📹 控制: {}",
                if device.has_volume_control && device.has_switch_control {
                    "音量+开关"
                } else if device.has_volume_control {
                    "仅音量"
                } else if device.has_switch_control {
                    "仅开关"
                } else {
                    "无"
                }
            )));
        });
    }

    /// Draw volume control window, returns true if window was closed
    pub fn draw_debug_display_window(&mut self, ctx: &egui::Context) {
        if !self.state.ui_state.show_debug_window {
            return;
        }

        let mut window_open = true;

        egui::Window::new("🐛 调试信息")
            .collapsible(false)
            .resizable(true)
            .default_width(400.0)
            .default_height(300.0)
            .open(&mut window_open)
            .show(ctx, |ui| {
                ui.label("📊 性能指标");
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

                ui.label("💻 系统状态");
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
                            egui::RichText::new(format!("{:.1}%", snapshot.memory_usage_percent))
                                .color(mem_color),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("运行时间:");
                        ui.label(self.state.system_monitor.get_uptime_string());
                    });
                }

                ui.separator();

                ui.label("🔊 音频系统");
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

                // 操作按钮
                ui.horizontal(|ui| {
                    if ui.small_button("🔄 刷新音频").clicked() {
                        if let Err(e) = self.state.audio_manager.refresh_devices() {
                            error!("Failed to refresh audio devices: {}", e);
                        }
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("❌ 关闭").clicked() {
                            self.state.ui_state.toggle_debug_window();
                        }
                    });
                });
            });

        if !window_open || ctx.input(|i| i.viewport().close_requested()) {
            self.state.ui_state.toggle_debug_window();
        }
    }

    /// Draw workspace information
    pub fn draw_workspace_panel(&mut self, ui: &mut egui::Ui) {
        let mut tag_status_vec = Vec::new();
        let mut layout_symbol = String::from(" ? ");
        let bold_thickness = 2.5;
        let light_thickness = 1.5;
        if let Some(ref message) = self.state.current_message {
            tag_status_vec = message.monitor_info.tag_status_vec.to_vec();
            layout_symbol = message.monitor_info.get_ltsymbol();
        }
        // Draw tag icons as buttons
        for (i, &tag_icon) in icons::TAG_ICONS.iter().enumerate() {
            let tag_color = colors::TAG_COLORS[i];
            let tag_bit = 1 << i;
            // 构建基础文本样式
            let mut rich_text = egui::RichText::new(tag_icon).monospace();
            // 设置工具提示文本
            let mut tooltip = format!("标签 {}", i + 1);
            // 根据状态设置样式
            if let Some(tag_status) = tag_status_vec.get(i) {
                if tag_status.is_filled {
                    tooltip.push_str(" (有窗口)");
                }
                // is_selected: 当前标签标记
                if tag_status.is_selected {
                    tooltip.push_str(" (当前)");
                }
                // is_urg: 紧急状态标记
                if tag_status.is_urg {
                    tooltip.push_str(" (紧急)");
                }
            }
            // 绘制各种装饰效果
            let mut is_urg = false;
            let mut is_filled = false;
            let mut is_selected = false;
            if let Some(tag_status) = tag_status_vec.get(i) {
                if tag_status.is_urg {
                    is_urg = true;
                    rich_text = rich_text.background_color(Color32::RED);
                } else if tag_status.is_filled {
                    is_filled = true;
                    let bg_color = Color32::from_rgba_premultiplied(
                        tag_color.r(),
                        tag_color.g(),
                        tag_color.b(),
                        255,
                    );
                    rich_text = rich_text.background_color(bg_color);
                } else if tag_status.is_selected {
                    is_selected = true;
                    let bg_color = Color32::from_rgba_premultiplied(
                        tag_color.r(),
                        tag_color.g(),
                        tag_color.b(),
                        210,
                    );
                    rich_text = rich_text.background_color(bg_color);
                } else if tag_status.is_occ {
                    let bg_color = Color32::from_rgba_premultiplied(
                        tag_color.r(),
                        tag_color.g(),
                        tag_color.b(),
                        180,
                    );
                    rich_text = rich_text.background_color(bg_color);
                } else {
                    rich_text = rich_text.background_color(Color32::TRANSPARENT);
                }
            }

            let label_response = ui.add(Button::new(rich_text).min_size(Vec2::new(36., 24.)));
            let rect = label_response.rect;
            self.state.ui_state.button_height = rect.height();
            if is_urg {
                ui.painter().rect_stroke(
                    rect,
                    1.0,
                    Stroke::new(bold_thickness, colors::VIOLET),
                    StrokeKind::Outside,
                );
            } else if is_filled {
                ui.painter().rect_stroke(
                    rect,
                    1.0,
                    Stroke::new(bold_thickness, tag_color),
                    StrokeKind::Outside,
                );
            } else if is_selected {
                ui.painter().rect_stroke(
                    rect,
                    1.0,
                    Stroke::new(light_thickness, tag_color),
                    StrokeKind::Outside,
                );
            }
            // 处理交互事件
            self.handle_tag_interactions(&label_response, tag_bit, i);

            // 悬停效果和工具提示
            if label_response.hovered() {
                ui.painter().rect_stroke(
                    rect.expand(1.0),
                    1.0,
                    Stroke::new(bold_thickness, tag_color),
                    StrokeKind::Outside,
                );
                label_response.on_hover_text(tooltip);
            }
        }

        self.render_layout_section(ui, &layout_symbol);
    }
    // 提取交互处理逻辑到单独函数
    fn handle_tag_interactions(
        &self,
        label_response: &egui::Response,
        tag_bit: u32,
        tag_index: usize,
    ) {
        // 左键点击 - ViewTag 命令
        if label_response.clicked() {
            info!("{} clicked", tag_bit);
            self.send_tag_command(tag_bit, tag_index, true);
        }

        // 右键点击 - ToggleTag 命令
        if label_response.secondary_clicked() {
            info!("{} secondary_clicked", tag_bit);
            self.send_tag_command(tag_bit, tag_index, false);
        }
    }

    // 提取命令发送逻辑
    fn send_tag_command(&self, tag_bit: u32, _tag_index: usize, is_view: bool) {
        if let Some(ref message) = self.state.current_message {
            let monitor_id = message.monitor_info.monitor_num;

            let command = if is_view {
                SharedCommand::view_tag(tag_bit, monitor_id)
            } else {
                SharedCommand::toggle_tag(tag_bit, monitor_id)
            };

            if let Some(shared_buffer) = &self.shared_buffer_opt {
                match shared_buffer.send_command(command) {
                    Ok(true) => {
                        info!("Sent command: {:?} by shared_buffer", command);
                    }
                    Ok(false) => {
                        warn!("Command buffer full, command dropped");
                    }
                    Err(e) => {
                        error!("Failed to send command: {}", e);
                    }
                }
            }
        }
    }

    fn send_layout_command(&mut self, layout_index: u32) {
        if let Some(ref message) = self.state.current_message {
            let monitor_id = message.monitor_info.monitor_num;
            let command = SharedCommand::new(CommandType::SetLayout, layout_index, monitor_id);

            if let Some(shared_buffer) = &self.shared_buffer_opt {
                match shared_buffer.send_command(command) {
                    Ok(true) => {
                        info!("Sent command: {:?} by shared_buffer", command);
                    }
                    Ok(false) => {
                        warn!("Command buffer full, command dropped");
                    }
                    Err(e) => {
                        error!("Failed to send command: {}", e);
                    }
                }
            }
        }
    }

    fn render_layout_section(&mut self, ui: &mut egui::Ui, layout_symbol: &str) {
        ui.separator();
        // 主布局按钮
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

        // 处理主布局按钮点击
        if main_layout_button.clicked() {
            info!("Layout button clicked, toggling selector");
            self.state.layout_selector_open = !self.state.layout_selector_open;
        }

        // 如果选择器是展开的，显示布局选项
        if self.state.layout_selector_open {
            ui.separator();

            // 水平显示所有布局选项
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

                // 处理布局选项点击
                if layout_option_button.clicked() && !is_current {
                    info!("Layout option clicked: {} ({})", layout.name, layout.symbol);
                    self.send_layout_command(layout.index);

                    // 选择后关闭选择器
                    self.state.layout_selector_open = false;
                }

                // 添加工具提示
                let hover_text = format!("点击切换布局: {}", layout.name);
                layout_option_button.on_hover_text(hover_text);
            }
        }
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
            if battery_percent < 0.2 * 100.0 && !is_charging {
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

        let label_response = ui.add(Button::new(volume_icon));
        if label_response.clicked() {
            self.state.ui_state.toggle_volume_window();
        }

        label_response.on_hover_text(tooltip);
    }

    /// Draw debug control button
    fn draw_debug_button(&mut self, ui: &mut egui::Ui) {
        let (debug_icon, tooltip) = if self.state.ui_state.show_debug_window {
            ("󰱭", "关闭调试窗口") // 激活状态的图标和提示
        } else {
            ("🔍", "打开调试窗口") // 默认状态的图标和提示
        };

        let label_response = ui.add(Button::new(debug_icon).sense(Sense::click()));
        if label_response.clicked() {
            self.state.ui_state.toggle_debug_window();
        }

        // 添加详细的悬停提示信息
        let _detailed_tooltip = format!(
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

        // label_response.on_hover_text(detailed_tooltip);
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

    fn draw_screenshot_button(&mut self, ui: &mut egui::Ui) {
        let label_response = ui.add(Button::new(format!(
            "{} {:.2}",
            icons::SCREENSHOT_ICON,
            self.state.ui_state.scale_factor
        )));

        if label_response.clicked() {
            let _ = Command::new("flameshot").arg("gui").spawn();
        }
    }

    fn draw_monitor_number(&mut self, ui: &mut egui::Ui) {
        if let Some(ref message) = self.state.current_message {
            let monitor_num = (message.monitor_info.monitor_num as usize).min(1);
            ui.add(Label::new(
                egui::RichText::new(format!("{}", icons::MONITOR_NUMBERS[monitor_num])).strong(),
            ));
        }
    }

    /// Draw constoller information panel
    pub fn draw_controller_info_panel(&mut self, ui: &mut egui::Ui) {
        // Battery info
        self.draw_battery_info(ui);

        // Volume button
        self.draw_volume_button(ui);

        // Debug button
        self.draw_debug_button(ui);

        // Time display
        self.draw_time_display(ui);

        // Screenshot button
        self.draw_screenshot_button(ui);

        // Monitor number
        self.draw_monitor_number(ui);
    }

    /// Draw system information panel
    pub fn draw_system_info_panel(&mut self, ui: &mut egui::Ui) {
        // Ensure color cache is initialized
        self.ensure_color_cache();

        // Memory information
        self.draw_memory_info(ui);

        // CPU chart
        self.draw_cpu_chart(ui);
    }

    fn draw_memory_info(&self, ui: &mut egui::Ui) {
        let (available_gb, used_gb) = self.state.get_memory_display_info();

        // Available memory
        ui.label(
            egui::RichText::new(format!("{:.1}G", available_gb)).color(colors::MEMORY_AVAILABLE),
        );

        // Used memory
        ui.label(egui::RichText::new(format!("{:.1}G", used_gb)).color(colors::MEMORY_USED));

        // Memory warning indicator
        if let Some(snapshot) = self.state.system_monitor.get_snapshot() {
            if snapshot.memory_usage_percent > 0.8 * 100.0 {
                ui.label("⚠️");
            }
        }
        ui.separator();
    }

    fn draw_cpu_chart(&mut self, ui: &mut egui::Ui) {
        // Reset button
        let reset_view = ui.add(Button::new("🔄"));

        // CPU usage indicator
        if let Some(snapshot) = self.state.system_monitor.get_snapshot() {
            let cpu_color = self.get_cpu_color(snapshot.cpu_average as f64 / 100.0);
            ui.label(
                egui::RichText::new(format!("{}%", snapshot.cpu_average as i32)).color(cpu_color),
            );

            // CPU warning indicator
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
            // Create plot points for all CPU cores
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

                // Draw individual CPU core points with different colors
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

                // Draw average line if we have multiple cores
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

    fn get_average_cpu_color(&self, cpu_data: &[f64]) -> Color32 {
        if cpu_data.is_empty() {
            return colors::CPU_LOW;
        }

        let avg_usage = cpu_data.iter().sum::<f64>() / cpu_data.len() as f64;
        self.get_cpu_color(avg_usage)
    }

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
}

impl eframe::App for EguiBarApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        START.call_once(|| {
            self.state.ui_state.need_resize = true;
        });
        ctx.set_pixels_per_point(self.state.ui_state.scale_factor);

        // Update application state (system monitoring, audio, etc.)
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

        // Draw main UI
        egui::CentralPanel::default()
            .frame(
                egui::Frame::default()
                    .fill(Color32::WHITE)
                    .inner_margin(egui::Margin::symmetric(8, 4)),
            )
            .show(ctx, |ui| {
                self.draw_main_ui(ui);

                // Draw volume control window
                self.draw_volume_control_window(ctx);

                // Draw debug display window
                self.draw_debug_display_window(ctx);

                // Adjust window if needed
                self.adjust_window(ctx, ui);
            });

        if self.state.ui_state.need_resize {
            info!("request for resize");
            ctx.request_repaint_after(std::time::Duration::from_millis(1));
        }
    }
}
