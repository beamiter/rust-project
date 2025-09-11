use eframe::{egui, App};
use image::DynamicImage;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::thread;
use walkdir::WalkDir;

// 支持的图像格式
const SUPPORTED_FORMATS: &[&str] = &["jpg", "jpeg", "png", "gif", "bmp", "webp", "tiff"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum FitMode {
    Custom, // 用户自由缩放/平移
    Fit,    // 适配窗口（不放大超过原图）
    Actual, // 1:1
}

pub struct ImageViewerApp {
    // 当前文件夹和图像信息
    current_folder: Option<PathBuf>,
    image_files: Vec<PathBuf>,
    current_image_index: usize,

    // 图像与纹理
    current_texture: Option<egui::TextureHandle>,
    current_image: Option<DynamicImage>,
    image_size: Option<[usize; 2]>,
    // 可选：简单纹理缓存（按需启用）
    // texture_cache: std::collections::HashMap<PathBuf, egui::TextureHandle>,

    // 缩放控制
    default_scale: f32,
    current_scale: f32,
    fit_mode: FitMode,

    // 异步加载
    image_rx: Option<Receiver<(DynamicImage, PathBuf)>>,
    loading_image_path: Option<PathBuf>,
    last_error: Option<String>,

    // 平移
    image_offset: egui::Vec2,
    is_dragging: bool,
    last_pointer_pos: Option<egui::Pos2>,

    // 左侧面板显示/隐藏
    show_file_panel: bool,
    pending_toggle_file_panel: bool,

    // 配置：是否需要按住Ctrl/Cmd才用滚轮缩放
    wheel_zoom_requires_modifier: bool,
}

impl Default for ImageViewerApp {
    fn default() -> Self {
        Self {
            current_folder: None,
            image_files: Vec::new(),
            current_image_index: 0,
            current_texture: None,
            current_image: None,
            default_scale: 1.0,
            current_scale: 1.0,
            fit_mode: FitMode::Custom,
            image_rx: None,
            loading_image_path: None,
            last_error: None,
            image_size: None,
            image_offset: egui::Vec2::ZERO,
            is_dragging: false,
            last_pointer_pos: None,
            show_file_panel: true,
            pending_toggle_file_panel: false,
            wheel_zoom_requires_modifier: true,
            // texture_cache: std::collections::HashMap::new(),
        }
    }
}

impl App for ImageViewerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 1. 处理异步加载
        self.check_image_loading(ctx);

        // 2. 键盘导航（尊重UI焦点：当UI需要键盘输入时不抢）
        if !ctx.wants_keyboard_input() {
            self.handle_keyboard_navigation(ctx);
        }

        // 3. 处理待切换文件面板（来自底部或悬浮按钮）
        if self.pending_toggle_file_panel {
            self.show_file_panel = !self.show_file_panel;
            self.pending_toggle_file_panel = false;
        }

        // 4. 渲染UI
        self.render_top_menu(ctx);
        if self.show_file_panel {
            self.render_file_panel(ctx);
        }
        self.render_status_bar(ctx);
        self.render_image_view(ctx);
    }
}

impl ImageViewerApp {
    // ============ 加载与数据处理 ============

    fn check_image_loading(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.image_rx {
            if let Ok((image, path)) = rx.try_recv() {
                // 只处理当前这次请求的结果
                if Some(&path) == self.loading_image_path.as_ref() {
                    let size = [image.width() as usize, image.height() as usize];

                    // 创建纹理
                    let image_buffer = image.to_rgba8();
                    let image_data = egui::ColorImage::from_rgba_unmultiplied(
                        [image.width() as _, image.height() as _],
                        &image_buffer.into_raw(),
                    );

                    let texture = ctx.load_texture(
                        path.file_name().unwrap_or_default().to_string_lossy(),
                        image_data,
                        egui::TextureOptions::default(),
                    );

                    self.current_texture = Some(texture);
                    self.current_image = Some(image);
                    self.image_size = Some(size);
                    self.loading_image_path = None;
                    self.last_error = None;

                    // 初始进入时可选择自动适配
                    match self.fit_mode {
                        FitMode::Fit => {
                            // 会在render_image_view中根据可用区域进行适配
                        }
                        FitMode::Actual => {
                            self.current_scale = 1.0;
                            self.image_offset = egui::Vec2::ZERO;
                        }
                        FitMode::Custom => {
                            self.current_scale = self.default_scale;
                            self.image_offset = egui::Vec2::ZERO;
                        }
                    }
                }
            }
        }
    }

    fn is_supported(path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|ext| {
                let ext = ext.to_lowercase();
                SUPPORTED_FORMATS.contains(&ext.as_str())
            })
            .unwrap_or(false)
    }

    fn handle_keyboard_navigation(&mut self, ctx: &egui::Context) {
        if self.image_files.is_empty() {
            return;
        }

        ctx.input(|i| {
            // 左右/上下
            if i.key_pressed(egui::Key::ArrowLeft) || i.key_pressed(egui::Key::ArrowUp) {
                self.navigate_to_previous_image();
            } else if i.key_pressed(egui::Key::ArrowRight) || i.key_pressed(egui::Key::ArrowDown) {
                self.navigate_to_next_image();
            }

            // F 切换文件面板
            if i.key_pressed(egui::Key::F) {
                self.show_file_panel = !self.show_file_panel;
            }

            // +/- 缩放
            if i.key_pressed(egui::Key::Equals) || i.key_pressed(egui::Key::Plus) {
                self.apply_zoom_centered(1.1);
            }
            if i.key_pressed(egui::Key::Minus) {
                self.apply_zoom_centered(0.9);
            }

            // 空格/Backspace 翻页
            if i.key_pressed(egui::Key::Space) {
                self.navigate_to_next_image();
            }
            if i.key_pressed(egui::Key::Backspace) {
                self.navigate_to_previous_image();
            }

            // 数字键：1为1:1，0为Fit
            if i.key_pressed(egui::Key::Num1) {
                self.fit_mode = FitMode::Actual;
                self.current_scale = 1.0;
                self.image_offset = egui::Vec2::ZERO;
            }
            if i.key_pressed(egui::Key::Num0) {
                self.fit_mode = FitMode::Fit;
                self.image_offset = egui::Vec2::ZERO;
            }
        });
    }

    fn navigate_to_previous_image(&mut self) {
        if self.image_files.is_empty() {
            return;
        }
        if self.current_image_index > 0 {
            self.current_image_index -= 1;
        } else {
            self.current_image_index = self.image_files.len() - 1;
        }
        let path = self.image_files[self.current_image_index].clone();
        self.load_image(path);
    }

    fn navigate_to_next_image(&mut self) {
        if self.image_files.is_empty() {
            return;
        }
        if self.current_image_index < self.image_files.len() - 1 {
            self.current_image_index += 1;
        } else {
            self.current_image_index = 0;
        }
        let path = self.image_files[self.current_image_index].clone();
        self.load_image(path);
    }

    // ============ UI 渲染 ============

    fn render_top_menu(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open Folder").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            self.load_folder(path);
                            ui.close();
                        }
                    }
                });

                ui.menu_button("View", |ui| {
                    ui.label("Default Scale");
                    ui.add(egui::Slider::new(&mut self.default_scale, 0.1..=5.0));

                    if ui.button("Apply Default Scale").clicked() {
                        self.fit_mode = FitMode::Custom;
                        self.current_scale = self.default_scale;
                        self.image_offset = egui::Vec2::ZERO;
                        ui.close();
                    }

                    if ui.button("Fit to Window").clicked() {
                        self.fit_mode = FitMode::Fit;
                        self.image_offset = egui::Vec2::ZERO;
                        ui.close();
                    }

                    if ui.button("Actual Size (100%)").clicked() {
                        self.fit_mode = FitMode::Actual;
                        self.current_scale = 1.0;
                        self.image_offset = egui::Vec2::ZERO;
                        ui.close();
                    }

                    if ui.button("Reset Position").clicked() {
                        self.image_offset = egui::Vec2::ZERO;
                        ui.close();
                    }

                    ui.separator();

                    let panel_text = if self.show_file_panel {
                        "Hide File Panel"
                    } else {
                        "Show File Panel"
                    };

                    if ui.button(panel_text).clicked() {
                        self.show_file_panel = !self.show_file_panel;
                        ui.close();
                    }

                    ui.separator();

                    ui.checkbox(
                        &mut self.wheel_zoom_requires_modifier,
                        "Wheel Zoom needs Ctrl/Cmd",
                    );
                });

                ui.menu_button("Navigation", |ui| {
                    if ui.button("Previous Image").clicked() {
                        self.navigate_to_previous_image();
                        ui.close();
                    }
                    if ui.button("Next Image").clicked() {
                        self.navigate_to_next_image();
                        ui.close();
                    }
                    ui.separator();
                    ui.label("Shortcuts:");
                    ui.label("left/up: Previous, right/down: Next");
                    ui.label("Space/Backspace: Next/Previous");
                    ui.label("F: Toggle File Panel");
                    ui.label("+/-: Zoom In/Out");
                    ui.label("0: Fit, 1: Actual");
                    ui.label("Drag: Pan, Wheel: Zoom (Ctrl/Cmd if enabled)");
                    ui.label("Double-Click: Toggle Fit/Actual");
                    ui.label("Right-Click: Context Menu");
                });

                // 显示当前文件名
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(current_file) = self.image_files.get(self.current_image_index) {
                        if let Some(file_name) = current_file.file_name() {
                            ui.label(file_name.to_string_lossy().to_string());
                        }
                    }
                });
            });
        });
    }

    fn render_file_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("file_panel")
            .resizable(true)
            .default_width(240.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Files");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("✖").clicked() {
                            self.show_file_panel = false;
                        }
                    });
                });

                if ui.button("Select Folder").clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                        self.load_folder(path);
                    }
                }

                ui.separator();

                if let Some(folder) = &self.current_folder {
                    ui.label(format!("Folder: {}", folder.to_string_lossy()));
                } else {
                    ui.label("No folder selected");
                }

                ui.separator();

                egui::ScrollArea::vertical().show(ui, |ui| {
                    let mut image_to_load: Option<(usize, PathBuf)> = None;

                    for (i, path) in self.image_files.iter().enumerate() {
                        let file_name = path.file_name().unwrap_or_default().to_string_lossy();
                        let is_selected = i == self.current_image_index;
                        if ui.selectable_label(is_selected, file_name).clicked() {
                            image_to_load = Some((i, path.clone()));
                        }
                    }

                    if let Some((i, path)) = image_to_load {
                        self.current_image_index = i;
                        self.load_image(path);
                    }
                });
            });
    }

    fn render_status_bar(&self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if !self.show_file_panel {
                    if ui.button("📁 Files").clicked() {
                        // 此处无法改self，标记待切换
                        ui.ctx().request_repaint(); // 保证下一帧
                                                    // 用一个提示：在下一帧中处理pending_toggle_file_panel
                                                    // 由于当前是不可变借用，不能直接改
                                                    // 我们改为：在中央面板里做一个小Hook（已实现）
                    }
                    ui.separator();
                }

                if let Some(size) = self.image_size {
                    ui.label(format!("Image: {}x{}", size[0], size[1]));
                } else {
                    ui.label("Image: -");
                }

                ui.label(format!("Zoom: {:.0}%", self.current_scale * 100.0));
                ui.label(format!(
                    "Offset: X={:.0}, Y={:.0}",
                    self.image_offset.x, self.image_offset.y
                ));

                if !self.image_files.is_empty() {
                    ui.label(format!(
                        "Image {}/{}",
                        self.current_image_index + 1,
                        self.image_files.len()
                    ));
                }

                if let Some(err) = &self.last_error {
                    ui.colored_label(egui::Color32::RED, format!("Error: {err}"));
                }
            });
        });
    }

    fn render_image_view(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let available = ui.available_size();
            let image_area_rect = egui::Rect::from_min_size(ui.min_rect().min, available);

            // 背景交互区域
            let bg_response = ui.interact(
                image_area_rect,
                ui.id().with("image_bg"),
                egui::Sense::click_and_drag(),
            );

            // 右键菜单
            if bg_response.secondary_clicked() {
                bg_response.context_menu(|ui| {
                    if ui.button("Fit to Window").clicked() {
                        self.fit_mode = FitMode::Fit;
                        self.image_offset = egui::Vec2::ZERO;
                        ui.close();
                    }
                    if ui.button("Actual Size (100%)").clicked() {
                        self.fit_mode = FitMode::Actual;
                        self.current_scale = 1.0;
                        self.image_offset = egui::Vec2::ZERO;
                        ui.close();
                    }
                    if ui.button("Reset Position").clicked() {
                        self.image_offset = egui::Vec2::ZERO;
                        self.fit_mode = FitMode::Custom;
                        ui.close();
                    }
                });
            }

            // 双击切换Fit/Actual
            if bg_response.double_clicked() {
                if self.fit_mode == FitMode::Actual {
                    self.fit_mode = FitMode::Fit;
                } else {
                    self.fit_mode = FitMode::Actual;
                }
                self.image_offset = egui::Vec2::ZERO;
            }

            // 只在有图像时处理缩放与拖拽
            if self.current_texture.is_some() && self.image_size.is_some() {
                // 先执行缩放（以鼠标为锚点）
                self.handle_mouse_zoom(ctx, ui, image_area_rect);

                // 如果当前是"Fit"模式，在这里根据可用区域适配scale
                if self.fit_mode == FitMode::Fit {
                    self.fit_image_to_window(available);
                }

                // 再处理平移（拖拽）
                self.handle_dragging(ctx, ui, image_area_rect);

                // 平移边界约束
                self.clamp_offset(available);

                // 绘制图像（根据缩放与偏移）
                self.draw_image(ui, image_area_rect);
            } else {
                if self.loading_image_path.is_some() {
                    ui.centered_and_justified(|ui| {
                        ui.label("Loading image...");
                    });
                } else if self.image_files.is_empty() && self.current_folder.is_some() {
                    ui.centered_and_justified(|ui| {
                        ui.label("No images found in this folder.");
                    });
                } else if self.current_folder.is_none() {
                    ui.centered_and_justified(|ui| {
                        ui.label("Select a folder to view images.");
                    });
                }
            }

            // 隐藏面板时，显示一个靠左的悬浮按钮
            if !self.show_file_panel {
                let button_rect = egui::Rect::from_min_size(
                    image_area_rect.left_top()
                        + egui::vec2(6.0, image_area_rect.height() * 0.5 - 20.0),
                    egui::vec2(24.0, 40.0),
                );
                let response = ui.allocate_rect(button_rect, egui::Sense::click());
                let color = if response.hovered() {
                    egui::Color32::from_rgba_premultiplied(90, 90, 90, 200)
                } else {
                    egui::Color32::from_rgba_premultiplied(70, 70, 70, 160)
                };
                ui.painter().rect_filled(button_rect, 6.0, color);
                ui.painter().text(
                    button_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "📁",
                    egui::FontId::proportional(16.0),
                    egui::Color32::WHITE,
                );
                if response.clicked() {
                    // 这里可以直接改，因为我们在&mut self闭包里
                    self.show_file_panel = true;
                }
            }
        });
    }

    fn draw_image(&self, ui: &mut egui::Ui, rect: egui::Rect) {
        if let (Some(texture), Some(size)) = (&self.current_texture, self.image_size) {
            let scaled_w = size[0] as f32 * self.current_scale;
            let scaled_h = size[1] as f32 * self.current_scale;

            // 将图像居中+偏移
            let center = rect.center();
            let top_left = center - egui::vec2(scaled_w, scaled_h) * 0.5 + self.image_offset;

            let image_rect = egui::Rect::from_min_size(top_left, egui::vec2(scaled_w, scaled_h));

            ui.painter().image(
                texture.id(),
                image_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
    }

    // ============ 交互：缩放与平移 ============

    fn handle_dragging(&mut self, ctx: &egui::Context, ui: &mut egui::Ui, image_area: egui::Rect) {
        let response = ui.interact(
            image_area,
            ui.id().with("image_drag_area"),
            egui::Sense::drag(),
        );
        let current_pos = ctx.pointer_hover_pos();

        if response.drag_started() {
            self.is_dragging = true;
            self.last_pointer_pos = current_pos;
            self.fit_mode = FitMode::Custom; // 拖拽后进入自定义模式
        }

        if self.is_dragging && response.dragged() {
            if let (Some(last_pos), Some(current_pos)) = (self.last_pointer_pos, current_pos) {
                let delta = current_pos - last_pos;
                self.image_offset += delta;
                self.last_pointer_pos = Some(current_pos);
            }
        }

        if response.drag_stopped() {
            self.is_dragging = false;
            self.last_pointer_pos = None;
        }
    }

    fn handle_mouse_zoom(
        &mut self,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        image_area: egui::Rect,
    ) {
        let pointer_in_area =
            image_area.contains(ui.input(|i| i.pointer.hover_pos().unwrap_or_default()));
        if !pointer_in_area {
            return;
        }

        ctx.input(|i| {
            let modifiers_ok = if self.wheel_zoom_requires_modifier {
                i.modifiers.ctrl || i.modifiers.command
            } else {
                true
            };

            if modifiers_ok {
                let scroll = i.raw_scroll_delta.y;
                if scroll.abs() > f32::EPSILON {
                    let factor = if scroll > 0.0 { 1.1 } else { 0.9 };
                    let pointer_pos = i.pointer.hover_pos();

                    if let Some(pointer_pos) = pointer_pos {
                        self.apply_zoom_with_anchor(factor, pointer_pos, image_area);
                        self.fit_mode = FitMode::Custom; // 一旦滚轮缩放，进入自定义模式
                    }
                }
            }
        });
    }

    fn apply_zoom_centered(&mut self, factor: f32) {
        // 没有指针坐标时，以视口中心为锚点的近似做法：保持偏移比例不变
        self.current_scale = (self.current_scale * factor).clamp(0.05, 20.0);
    }

    fn apply_zoom_with_anchor(
        &mut self,
        factor: f32,
        anchor_screen_pos: egui::Pos2,
        viewport: egui::Rect,
    ) {
        if let Some(_size) = self.image_size {
            let pre_scale = self.current_scale;
            let new_scale = (self.current_scale * factor).clamp(0.05, 20.0);
            if (new_scale - pre_scale).abs() < f32::EPSILON {
                return;
            }

            // 计算图像中心（屏幕坐标）
            let view_center = viewport.center();
            let img_center = view_center + self.image_offset;

            // 当前缩放下，锚点对应的图像坐标
            let dx = (anchor_screen_pos.x - img_center.x) / pre_scale;
            let dy = (anchor_screen_pos.y - img_center.y) / pre_scale;

            // 新缩放后，为了让同一图像点仍然在anchor_screen_pos，调整offset
            self.current_scale = new_scale;
            let new_img_center_x = anchor_screen_pos.x - dx * new_scale;
            let new_img_center_y = anchor_screen_pos.y - dy * new_scale;

            let new_center = egui::pos2(new_img_center_x, new_img_center_y);
            self.image_offset = new_center - view_center;
        }
    }

    fn clamp_offset(&mut self, viewport_size: egui::Vec2) {
        if let Some(size) = self.image_size {
            let scaled_w = size[0] as f32 * self.current_scale;
            let scaled_h = size[1] as f32 * self.current_scale;

            // 如果图像小于视口，则保持居中（offset为0）
            let max_x = (scaled_w - viewport_size.x) * 0.5;
            let max_y = (scaled_h - viewport_size.y) * 0.5;

            if scaled_w <= viewport_size.x {
                self.image_offset.x = 0.0;
            } else {
                self.image_offset.x = self.image_offset.x.clamp(-max_x, max_x);
            }

            if scaled_h <= viewport_size.y {
                self.image_offset.y = 0.0;
            } else {
                self.image_offset.y = self.image_offset.y.clamp(-max_y, max_y);
            }
        } else {
            self.image_offset = egui::Vec2::ZERO;
        }
    }

    fn fit_image_to_window(&mut self, available_size: egui::Vec2) {
        if let Some(size) = self.image_size {
            let scale_x = available_size.x / size[0] as f32;
            let scale_y = available_size.y / size[1] as f32;
            // 不放大超过原始大小
            self.current_scale = scale_x.min(scale_y).min(1.0).max(0.01);
            self.image_offset = egui::Vec2::ZERO;
        }
    }

    // ============ 加载文件夹/图像 ============

    fn load_folder(&mut self, path: PathBuf) {
        self.current_folder = Some(path.clone());
        self.image_files.clear();
        self.current_image_index = 0;
        self.current_texture = None;
        self.current_image = None;
        self.image_offset = egui::Vec2::ZERO;
        self.last_error = None;
        // self.texture_cache.clear();

        for entry in WalkDir::new(path)
            .max_depth(1)
            .into_iter()
            .filter_map(Result::ok)
        {
            let p = entry.path().to_path_buf();
            if p.is_file() && Self::is_supported(&p) {
                self.image_files.push(p);
            }
        }

        // 按文件名排序
        self.image_files
            .sort_by(|a, b| a.file_name().unwrap().cmp(b.file_name().unwrap()));

        if !self.image_files.is_empty() {
            let first = self.image_files[0].clone();
            self.load_image(first);
        }

        self.show_file_panel = true;
    }

    fn load_image(&mut self, path: PathBuf) {
        // 取消之前的加载
        self.image_rx = None;
        self.loading_image_path = Some(path.clone());
        self.last_error = None;

        // 清理当前纹理/图像显示（可保留上一张直到新图加载完成：按需选择）
        self.current_texture = None;
        self.current_image = None;
        self.image_size = None;

        // 平移复位（缩放模式维持）
        self.image_offset = egui::Vec2::ZERO;

        // 如果想启用纹理缓存，可在此检测缓存
        // if let Some(tex) = self.texture_cache.get(&path).cloned() {
        //     self.current_texture = Some(tex);
        //     // 注意：需要在缓存里保存图像尺寸
        //     // 此处略，建议并存一个metadata
        //     self.loading_image_path = None;
        //     return;
        // }

        let (tx, rx) = channel();
        self.image_rx = Some(rx);

        thread::spawn(move || {
            match image::open(&path) {
                Ok(img) => {
                    let _ = tx.send((img, path));
                }
                Err(e) => {
                    eprintln!("Failed to open image: {e}");
                    // 若要向UI传递错误，可增加一个错误通道
                }
            }
        });
    }
}
