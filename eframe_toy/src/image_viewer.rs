use eframe::{egui, App, CreationContext};
use image::DynamicImage;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::thread;
use walkdir::WalkDir;

// 支持的图像格式
const SUPPORTED_FORMATS: &[&str] = &["jpg", "jpeg", "png", "gif", "bmp", "webp", "tiff"];

pub struct ImageViewerApp {
    // 当前文件夹和图像信息
    current_folder: Option<PathBuf>,
    image_files: Vec<PathBuf>,
    current_image_index: usize,

    // 图像显示相关
    current_texture: Option<egui::TextureHandle>,
    current_image: Option<DynamicImage>,
    image_size: Option<[usize; 2]>,

    // 缩放控制
    default_scale: f32,
    current_scale: f32,

    // 异步加载相关
    image_rx: Option<Receiver<(DynamicImage, PathBuf)>>,
    loading_image_path: Option<PathBuf>,
    
    // 图像平移相关
    image_offset: egui::Vec2,
    is_dragging: bool,
    last_pointer_pos: Option<egui::Pos2>,
    
    // 新增：控制左侧面板显示/隐藏
    show_file_panel: bool,
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
            image_rx: None,
            loading_image_path: None,
            image_size: None,
            image_offset: egui::Vec2::ZERO,
            is_dragging: false,
            last_pointer_pos: None,
            // 默认显示文件面板
            show_file_panel: true,
        }
    }
}

impl App for ImageViewerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 检查是否有新加载的图像
        self.check_image_loading(ctx);
        
        // 处理键盘导航
        self.handle_keyboard_navigation(ctx);

        // 渲染UI
        self.render_top_menu(ctx);
        
        // 仅当show_file_panel为true时渲染左侧面板
        if self.show_file_panel {
            self.render_file_panel(ctx);
        }
        
        self.render_status_bar(ctx);
        self.render_image_view(ctx);
    }
}

impl ImageViewerApp {
    fn new(_cc: &CreationContext) -> Self {
        Self::default()
    }

    fn check_image_loading(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.image_rx {
            if let Ok((image, path)) = rx.try_recv() {
                // 更新当前图像
                self.current_image = Some(image.clone());
                self.loading_image_path = None;

                // 获取图像尺寸
                self.image_size = Some([image.width() as usize, image.height() as usize]);

                // 将图像转换为RGBA8并创建纹理
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
                self.current_scale = self.default_scale;
                
                // 重置图像偏移
                self.image_offset = egui::Vec2::ZERO;
            }
        }
    }
    
    fn handle_keyboard_navigation(&mut self, ctx: &egui::Context) {
        if !self.image_files.is_empty() {
            ctx.input(|i| {
                // 左右键切换图片
                if i.key_pressed(egui::Key::ArrowLeft) || i.key_pressed(egui::Key::ArrowUp) {
                    self.navigate_to_previous_image();
                } else if i.key_pressed(egui::Key::ArrowRight) || i.key_pressed(egui::Key::ArrowDown) {
                    self.navigate_to_next_image();
                }
                
                // 新增：按F键切换文件面板显示/隐藏
                if i.key_pressed(egui::Key::F) {
                    self.show_file_panel = !self.show_file_panel;
                }
            });
        }
    }
    
    fn navigate_to_previous_image(&mut self) {
        if !self.image_files.is_empty() {
            if self.current_image_index > 0 {
                self.current_image_index -= 1;
            } else {
                self.current_image_index = self.image_files.len() - 1; // 循环到最后一张
            }
            self.load_image(self.image_files[self.current_image_index].clone());
        }
    }
    
    fn navigate_to_next_image(&mut self) {
        if !self.image_files.is_empty() {
            if self.current_image_index < self.image_files.len() - 1 {
                self.current_image_index += 1;
            } else {
                self.current_image_index = 0; // 循环到第一张
            }
            self.load_image(self.image_files[self.current_image_index].clone());
        }
    }

    fn render_top_menu(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open Folder").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            self.load_folder(path);
                            ui.close_menu();
                        }
                    }
                });

                ui.menu_button("View", |ui| {
                    ui.add(
                        egui::Slider::new(&mut self.default_scale, 0.1..=5.0).text("Default Scale"),
                    );

                    if ui.button("Apply Default Scale").clicked() {
                        self.current_scale = self.default_scale;
                        ui.close_menu();
                    }

                    if ui.button("Fit to Window").clicked() {
                        self.fit_image_to_window(ui.available_size());
                        ui.close_menu();
                    }

                    if ui.button("Reset Zoom (100%)").clicked() {
                        self.current_scale = 1.0;
                        ui.close_menu();
                    }
                    
                    if ui.button("Reset Position").clicked() {
                        self.image_offset = egui::Vec2::ZERO;
                        ui.close_menu();
                    }
                    
                    ui.separator();
                    
                    // 新增：切换文件面板选项
                    let panel_text = if self.show_file_panel {
                        "Hide File Panel"
                    } else {
                        "Show File Panel"
                    };
                    
                    if ui.button(panel_text).clicked() {
                        self.show_file_panel = !self.show_file_panel;
                        ui.close_menu();
                    }
                });
                
                ui.menu_button("Navigation", |ui| {
                    if ui.button("Previous Image").clicked() {
                        self.navigate_to_previous_image();
                        ui.close_menu();
                    }
                    
                    if ui.button("Next Image").clicked() {
                        self.navigate_to_next_image();
                        ui.close_menu();
                    }
                    
                    ui.separator();
                    
                    ui.label("Keyboard Shortcuts:");
                    ui.label("← / ↑: Previous Image");
                    ui.label("→ / ↓: Next Image");
                    ui.label("F: Toggle File Panel");
                    ui.label("Drag: Move Image");
                    ui.label("Scroll: Zoom In/Out");
                });
                
                // 显示当前文件名称
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
            .default_width(200.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Files");
                    
                    // 添加一个右对齐的关闭按钮
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

                // 显示当前文件夹
                if let Some(folder) = &self.current_folder {
                    ui.label(format!("Folder: {}", folder.to_string_lossy()));
                } else {
                    ui.label("No folder selected");
                }

                ui.separator();

                // 文件列表
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let mut image_to_load = None;
                    for (i, path) in self.image_files.iter().enumerate() {
                        let file_name = path.file_name().unwrap().to_string_lossy();
                        let is_selected = i == self.current_image_index;
                        if ui.selectable_label(is_selected, file_name).clicked() {
                            image_to_load = Some((i, path.clone()));
                            break;
                        }
                    }
                    // 然后在循环外加载
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
                // 新增：如果文件面板隐藏，添加一个按钮来显示它
                if !self.show_file_panel {
                    if ui.button("📁 Files").clicked() {
                        // 我们不能在这里直接修改self.show_file_panel，因为self是不可变的
                        // 但我们可以设置一个持久值，然后在下一帧中读取
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(egui::Id::new("show_file_panel_next"), true);
                        });
                    }
                    ui.separator();
                }
                
                // 显示当前图像信息
                if let Some(size) = self.image_size {
                    ui.label(format!("Image Size: {}x{}", size[0], size[1]));
                }

                ui.label(format!("Zoom: {:.0}%", self.current_scale * 100.0));
                
                ui.label(format!(
                    "Offset: X={:.0}, Y={:.0}", 
                    self.image_offset.x, 
                    self.image_offset.y
                ));

                // 显示当前图像索引
                if !self.image_files.is_empty() {
                    ui.label(format!(
                        "Image {}/{}",
                        self.current_image_index + 1,
                        self.image_files.len()
                    ));
                }
            });
        });
        
        // 检查是否需要在下一帧显示文件面板
        ctx.data_mut(|data| {
            if data.get_temp(egui::Id::new("show_file_panel_next")).unwrap_or(false) {
                data.remove::<bool>(egui::Id::new("show_file_panel_next"));
                // 这里不能直接修改self.show_file_panel，因为self是不可变的
                // 所以我们设置另一个临时值，在update中处理
                data.insert_temp(egui::Id::new("toggle_file_panel"), true);
            }
        });
    }

    fn render_image_view(&mut self, ctx: &egui::Context) {
        // 检查是否需要切换文件面板显示状态
        ctx.data_mut(|data| {
            if data.get_temp(egui::Id::new("toggle_file_panel")).unwrap_or(false) {
                data.remove::<bool>(egui::Id::new("toggle_file_panel"));
                self.show_file_panel = !self.show_file_panel;
            }
        });
        
        egui::CentralPanel::default().show(ctx, |ui| {
            // 处理鼠标滚轮缩放
            self.handle_mouse_zoom(ctx, ui);
            
            // 处理拖拽
            self.handle_dragging(ctx, ui);

            // 显示图像
            if let Some(texture) = &self.current_texture {
                if let Some(size) = self.image_size {
                    // 计算缩放后的图像大小
                    let scaled_width = (size[0] as f32 * self.current_scale).round() as f32;
                    let scaled_height = (size[1] as f32 * self.current_scale).round() as f32;

                    // 居中显示图像，并应用偏移
                    let available_size = ui.available_size();
                    let x = (available_size.x - scaled_width) / 2.0 + self.image_offset.x;
                    let y = (available_size.y - scaled_height) / 2.0 + self.image_offset.y;

                    // 显示图像
                    ui.painter().image(
                        texture.id(),
                        egui::Rect::from_min_size(
                            ui.min_rect().min + egui::vec2(x, y),
                            egui::vec2(scaled_width, scaled_height),
                        ),
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }
            } else if self.loading_image_path.is_some() {
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
            
            // 如果文件面板隐藏，在左侧显示一个小按钮以便快速显示面板
            if !self.show_file_panel {
                let button_rect = egui::Rect::from_min_size(
                    ui.min_rect().min + egui::vec2(5.0, ui.available_size().y / 2.0 - 20.0),
                    egui::vec2(20.0, 40.0),
                );
                
                let response = ui.allocate_rect(button_rect, egui::Sense::click());
                if response.hovered() {
                    ui.painter().rect_filled(
                        button_rect,
                        5.0,
                        egui::Color32::from_rgba_premultiplied(100, 100, 100, 200),
                    );
                } else {
                    ui.painter().rect_filled(
                        button_rect,
                        5.0,
                        egui::Color32::from_rgba_premultiplied(80, 80, 80, 150),
                    );
                }
                
                ui.painter().text(
                    button_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "📁",
                    egui::FontId::proportional(16.0),
                    egui::Color32::WHITE,
                );
                
                if response.clicked() {
                    self.show_file_panel = true;
                }
            }
        });
    }
    
    fn handle_dragging(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let response = ui.interact(
            ui.max_rect(),
            ui.id().with("image_drag_area"),
            egui::Sense::drag(),
        );
        
        // 获取当前鼠标位置
        let current_pos = ctx.pointer_hover_pos();
        
        // 处理鼠标按下事件
        if response.drag_started() {
            self.is_dragging = true;
            self.last_pointer_pos = current_pos;
        }
        
        // 处理拖拽中的移动
        if self.is_dragging && response.dragged() {
            if let (Some(last_pos), Some(current_pos)) = (self.last_pointer_pos, current_pos) {
                // 计算移动差值并更新偏移量
                let delta = current_pos - last_pos;
                self.image_offset += delta;
                self.last_pointer_pos = Some(current_pos);
            }
        }
        
        // 处理鼠标释放事件
        if response.drag_released() {
            self.is_dragging = false;
            self.last_pointer_pos = None;
        }
    }

    fn handle_mouse_zoom(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        if ui.ui_contains_pointer() {
            ctx.input(|i| {
                let scroll_delta = i.raw_scroll_delta.y;
                if scroll_delta != 0.0 {
                    // 使用鼠标滚轮缩放图像
                    let zoom_factor = if scroll_delta > 0.0 { 1.1 } else { 0.9 };
                    self.current_scale *= zoom_factor;
                    // 限制缩放范围
                    self.current_scale = self.current_scale.clamp(0.1, 10.0);
                }
            });
        }
    }

    fn fit_image_to_window(&mut self, available_size: egui::Vec2) {
        if let Some(size) = self.image_size {
            let scale_x = available_size.x / size[0] as f32;
            let scale_y = available_size.y / size[1] as f32;
            self.current_scale = scale_x.min(scale_y).min(1.0); // 不放大超过原始大小
            
            // 重置偏移，使图像居中
            self.image_offset = egui::Vec2::ZERO;
        }
    }

    fn load_folder(&mut self, path: PathBuf) {
        self.current_folder = Some(path.clone());
        self.image_files.clear();
        self.current_image_index = 0;
        self.current_texture = None;
        self.current_image = None;
        
        // 重置偏移
        self.image_offset = egui::Vec2::ZERO;

        // 收集所有图像文件
        for entry in WalkDir::new(path)
            .max_depth(1)
            .into_iter()
            .filter_map(Result::ok)
        {
            let path = entry.path();
            if path.is_file() {
                if let Some(extension) = path.extension() {
                    let ext = extension.to_string_lossy().to_lowercase();
                    if SUPPORTED_FORMATS.iter().any(|&format| format == ext) {
                        self.image_files.push(path.to_path_buf());
                    }
                }
            }
        }

        // 按文件名排序
        self.image_files
            .sort_by(|a, b| a.file_name().unwrap().cmp(b.file_name().unwrap()));

        // 如果有图像文件，加载第一个
        if !self.image_files.is_empty() {
            self.load_image(self.image_files[0].clone());
        }
        
        // 确保文件面板显示，这样用户可以看到加载的文件
        self.show_file_panel = true;
    }

    fn load_image(&mut self, path: PathBuf) {
        // 取消之前的加载
        self.image_rx = None;

        // 记录正在加载的图像路径
        self.loading_image_path = Some(path.clone());
        
        // 重置偏移
        self.image_offset = egui::Vec2::ZERO;

        // 在后台线程中加载图像，避免阻塞UI
        let (tx, rx) = channel();
        self.image_rx = Some(rx);

        thread::spawn(move || {
            if let Ok(image) = image::open(&path) {
                let _ = tx.send((image, path));
            }
        });
    }
}
