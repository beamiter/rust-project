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
        }
    }
}

impl App for ImageViewerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 检查是否有新加载的图像
        self.check_image_loading(ctx);

        // 渲染UI
        self.render_top_menu(ctx);
        self.render_file_panel(ctx);
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
            }
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
                });
            });
        });
    }

    fn render_file_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("file_panel")
            .resizable(true)
            .default_width(200.0)
            .show(ctx, |ui| {
                ui.heading("Files");

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
                // 显示当前图像信息
                if let Some(size) = self.image_size {
                    ui.label(format!("Image Size: {}x{}", size[0], size[1]));
                }

                ui.label(format!("Zoom: {:.0}%", self.current_scale * 100.0));

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
    }

    fn render_image_view(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            // 处理鼠标滚轮缩放
            self.handle_mouse_zoom(ctx, ui);

            // 显示图像
            if let Some(texture) = &self.current_texture {
                if let Some(size) = self.image_size {
                    // 计算缩放后的图像大小
                    let scaled_width = (size[0] as f32 * self.current_scale).round() as f32;
                    let scaled_height = (size[1] as f32 * self.current_scale).round() as f32;

                    // 居中显示图像
                    let available_size = ui.available_size();
                    let x = (available_size.x - scaled_width) / 2.0;
                    let y = (available_size.y - scaled_height) / 2.0;

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
        });
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
        }
    }

    fn load_folder(&mut self, path: PathBuf) {
        self.current_folder = Some(path.clone());
        self.image_files.clear();
        self.current_image_index = 0;
        self.current_texture = None;
        self.current_image = None;

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
    }

    fn load_image(&mut self, path: PathBuf) {
        // 取消之前的加载
        self.image_rx = None;

        // 记录正在加载的图像路径
        self.loading_image_path = Some(path.clone());

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
