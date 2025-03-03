use arboard::Clipboard;
use copypasta::{x11_clipboard::X11ClipboardContext, ClipboardContext};
use device_query::{DeviceQuery, DeviceState};
use egui::Widget;
use enigo::Coordinate::Abs;
use enigo::{Enigo, Mouse, Settings};
use image::GenericImageView;
use image::{DynamicImage, ImageBuffer, ImageFormat, ImageReader, Rgba};
use rayon::prelude::*;
use std::error::Error;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;
use std::{fs, path::Path, process::Command};

use crate::ScreenSelection;

#[allow(dead_code)]
pub struct ImageProcessor {
    images: Vec<DynamicImage>,
    max_width: u32,
    total_height: u32,
    save_path: String,
    file_name: String,
    file_prefix: usize,
    paths: Vec<String>,
    image_log: String,
    adding_on_progress: bool,
    str_clipboard: X11ClipboardContext,
    image_clipboard: Clipboard,
    text: String,
    image_output_file: PathBuf,
    texture: Option<egui::TextureHandle>,
    corner_points: Vec<(i32, i32)>,
    corner_dx: i32,
    corner_dy: i32,
    device_state: DeviceState,
    enigo: Enigo,
    start_checkbox_pos: (i32, i32),
    scroll_num: i32,
    selection: Option<ScreenSelection>,
    start_button_text: String,
}
impl Default for ImageProcessor {
    fn default() -> Self {
        Self {
            images: Vec::new(),
            max_width: 0,
            total_height: 0,
            save_path: "/tmp/image_dir/".to_string(),
            file_name: String::from("output.png"),
            file_prefix: 0,
            paths: vec![],
            image_log: String::new(),
            adding_on_progress: false,
            str_clipboard: ClipboardContext::new().unwrap(),
            image_clipboard: Clipboard::new().unwrap(),
            text: String::new(),
            image_output_file: PathBuf::new(),
            texture: None,
            corner_points: Vec::new(),
            corner_dx: 0,
            corner_dy: 0,
            device_state: DeviceState::new(),
            enigo: Enigo::new(&Settings::default()).unwrap(),
            start_checkbox_pos: (0, 0),
            scroll_num: 10,
            selection: None,
            start_button_text: "selection".to_string(),
        }
    }
}

impl ImageProcessor {
    fn find_overlapping_region_ncc(
        &mut self,
        img1: &DynamicImage,
        img2: &DynamicImage,
        max_overlap_height: Option<u32>,
    ) -> Option<u32> {
        let (width1, height1) = img1.dimensions();
        let (width2, height2) = img2.dimensions();

        let compare_width = width1.min(width2);

        let max_overlap = max_overlap_height.unwrap_or_else(|| height1.min(height2).min(500));
        let min_overlap = 10;

        let mut best_match_height = 0;
        let mut best_match_score = -1.0;

        let results: Vec<(u32, f64)> = (min_overlap..=max_overlap)
            .into_par_iter()
            .map(|overlap_height| {
                let bottom_rows = height1 - overlap_height;

                let mut img1_data = Vec::with_capacity((overlap_height * compare_width) as usize);
                let mut img2_data = Vec::with_capacity((overlap_height * compare_width) as usize);

                for y in 0..overlap_height {
                    for x in 0..compare_width {
                        let pixel1 = img1.get_pixel(x, bottom_rows + y);
                        let pixel2 = img2.get_pixel(x, y);

                        let luma1 = 0.299 * pixel1[0] as f64
                            + 0.587 * pixel1[1] as f64
                            + 0.114 * pixel1[2] as f64;
                        let luma2 = 0.299 * pixel2[0] as f64
                            + 0.587 * pixel2[1] as f64
                            + 0.114 * pixel2[2] as f64;

                        img1_data.push(luma1);
                        img2_data.push(luma2);
                    }
                }

                let mean1 = img1_data.iter().sum::<f64>() / img1_data.len() as f64;
                let mean2 = img2_data.iter().sum::<f64>() / img2_data.len() as f64;

                let mut numerator = 0.0;
                let mut denom1 = 0.0;
                let mut denom2 = 0.0;

                for i in 0..img1_data.len() {
                    let diff1 = img1_data[i] - mean1;
                    let diff2 = img2_data[i] - mean2;

                    numerator += diff1 * diff2;
                    denom1 += diff1 * diff1;
                    denom2 += diff2 * diff2;
                }

                let ncc = if denom1 > 0.0 && denom2 > 0.0 {
                    numerator / (denom1.sqrt() * denom2.sqrt())
                } else {
                    -1.0
                };

                (overlap_height, ncc)
            })
            .collect();

        for (height, score) in results {
            if score > best_match_score && score > 0.9 {
                // 0.9是相关性阈值，可以调整
                best_match_score = score;
                best_match_height = height;
            }
        }

        if best_match_height > 0 {
            Some(best_match_height)
        } else {
            None
        }
    }

    fn stitch_images_with_blend(
        &mut self,
        img1: &DynamicImage,
        img2: &DynamicImage,
        overlap_height: u32,
    ) -> DynamicImage {
        let (width1, height1) = img1.dimensions();
        let (width2, height2) = img2.dimensions();

        let result_width = width1.max(width2);
        let result_height = height1 + height2 - overlap_height;

        let mut result = ImageBuffer::new(result_width, result_height);

        for y in 0..height1 - overlap_height {
            for x in 0..width1 {
                result.put_pixel(x, y, img1.get_pixel(x, y));
            }
        }

        for y in 0..overlap_height {
            let blend_factor = y as f32 / overlap_height as f32;
            let img1_y = height1 - overlap_height + y;
            let img2_y = y;

            for x in 0..result_width {
                if x < width1 && x < width2 {
                    let pixel1 = img1.get_pixel(x, img1_y);
                    let pixel2 = img2.get_pixel(x, img2_y);

                    let blended = Rgba([
                        ((1.0 - blend_factor) * pixel1[0] as f32 + blend_factor * pixel2[0] as f32)
                            as u8,
                        ((1.0 - blend_factor) * pixel1[1] as f32 + blend_factor * pixel2[1] as f32)
                            as u8,
                        ((1.0 - blend_factor) * pixel1[2] as f32 + blend_factor * pixel2[2] as f32)
                            as u8,
                        ((1.0 - blend_factor) * pixel1[3] as f32 + blend_factor * pixel2[3] as f32)
                            as u8,
                    ]);

                    result.put_pixel(x, height1 - overlap_height + y, blended);
                } else if x < width1 {
                    result.put_pixel(x, height1 - overlap_height + y, img1.get_pixel(x, img1_y));
                } else if x < width2 {
                    result.put_pixel(x, height1 - overlap_height + y, img2.get_pixel(x, img2_y));
                }
            }
        }

        for y in overlap_height..height2 {
            for x in 0..width2 {
                result.put_pixel(x, height1 - overlap_height + y, img2.get_pixel(x, y));
            }
        }

        DynamicImage::ImageRgba8(result)
    }

    fn stitch_multiple_images_parallel(
        &mut self,
        image_paths: &[PathBuf],
        max_overlap_height: Option<u32>,
        use_blend: bool,
    ) -> Result<DynamicImage, Box<dyn Error>> {
        if image_paths.is_empty() {
            return Err("empty image paths".into());
        }

        println!("正在加载图片...");

        let images: Vec<DynamicImage> = image_paths
            .par_iter()
            .map(|path| {
                println!("加载图片: {}", path.display());
                image::open(path).map_err(|e| format!("无法加载图片 {}: {}", path.display(), e))
            })
            .collect::<Result<Vec<_>, _>>()?;

        if images.len() == 1 {
            return Ok(images[0].clone());
        }

        println!("开始拼接图片...");

        let mut result = images[0].clone();

        for i in 1..images.len() {
            println!("拼接图片 {}/{}", i, images.len() - 1);

            let overlap_height =
                match self.find_overlapping_region_ncc(&result, &images[i], max_overlap_height) {
                    Some(height) => {
                        println!("  检测到重叠高度: {} 像素", height);
                        height
                    }
                    None => {
                        println!("  未检测到明显重叠，将直接拼接");
                        0
                    }
                };

            if use_blend && overlap_height > 0 {
                result = self.stitch_images_with_blend(&result, &images[i], overlap_height);
            } else {
                result = self.stitch_images(&result, &images[i], overlap_height);
            }
        }

        println!("拼接完成!");

        Ok(result)
    }
    fn stitch_images(
        &mut self,
        img1: &DynamicImage,
        img2: &DynamicImage,
        overlap_height: u32,
    ) -> DynamicImage {
        let (width1, height1) = img1.dimensions();
        let (width2, height2) = img2.dimensions();

        let result_width = width1.max(width2);
        let result_height = height1 + height2 - overlap_height;

        let mut result = ImageBuffer::new(result_width, result_height);

        for y in 0..height1 {
            for x in 0..width1 {
                result.put_pixel(x, y, img1.get_pixel(x, y));
            }
        }

        for y in overlap_height..height2 {
            for x in 0..width2 {
                result.put_pixel(x, height1 + y - overlap_height, img2.get_pixel(x, y));
            }
        }

        DynamicImage::ImageRgba8(result)
    }

    fn concatenate_images(&mut self, output_path: &PathBuf) -> Result<(), Box<dyn Error>> {
        let max_overlap_height = None;
        let use_blend = false;
        let mut image_paths = Vec::new();
        for dir in &self.paths {
            image_paths.push(PathBuf::from(dir));
        }

        println!("将处理 {} 张图片", image_paths.len());
        if let Some(max_height) = max_overlap_height {
            println!("max_height: {} 像素", max_height);
        }
        if use_blend {
            println!("use blend");
        }

        // 执行图片拼接
        let result =
            self.stitch_multiple_images_parallel(&image_paths, max_overlap_height, use_blend)?;

        // 保存结果
        println!("正在保存结果到: {}", output_path.display());
        result.save(&output_path)?;
        println!("保存完成!");

        Ok(())
    }
    #[allow(dead_code)]
    fn select_positions(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut left_button_was_pressed = false;
        self.corner_points.clear();
        loop {
            let keys = self.device_state.get_keys();
            let mouse = self.device_state.get_mouse();

            if keys.contains(&device_query::Keycode::Escape) {
                println!("cancel");
                break;
            }
            let left_button_pressed = mouse.button_pressed[1];
            if left_button_pressed && !left_button_was_pressed {
                let coords = mouse.coords;
                self.corner_points.push(coords);
                println!(
                    "pnt #{}: ({}, {})",
                    self.corner_points.len(),
                    coords.0,
                    coords.1
                );
                if self.corner_points.len() >= 2 {
                    self.display_results()?;
                    break;
                }
            }
            left_button_was_pressed = left_button_pressed;
            thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }

    fn display_results(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let ref points = self.corner_points;
        if points.len() < 2 {
            return Ok(());
        }
        println!("\n=== 记录结果 ===");
        println!("pnt1: ({}, {})", points[0].0, points[0].1);
        println!("pnt2: ({}, {})", points[1].0, points[1].1);
        let dx = points[1].0 - points[0].0;
        let dy = points[1].1 - points[0].1;
        assert!(dx >= 0);
        assert!(dy >= 0);
        let distance = ((dx * dx + dy * dy) as f64).sqrt();
        println!("dx: {} 像素", dx.abs());
        println!("dy: {} 像素", dy.abs());
        println!("distance: {:.2} 像素", distance);
        self.corner_dx = dx;
        self.corner_dy = dy;
        Ok(())
    }
    #[allow(dead_code)]
    fn load_image_from_path(
        &mut self,
        path: &std::path::Path,
        ctx: &egui::Context,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let image = ImageReader::open(path)?.decode()?;
        let texture = ctx.load_texture(
            "my_image",
            egui::ColorImage::from_rgba_unmultiplied(
                [image.width() as usize, image.height() as usize],
                image.to_rgba8().as_raw(),
            ),
            Default::default(),
        );
        self.texture = Some(texture);
        Ok(())
    }

    pub fn reset(&mut self) {
        self.images.clear();
        self.max_width = 0;
        self.total_height = 0;
        self.file_prefix = 0;
        self.paths.clear();
        self.image_log.clear();
        self.adding_on_progress = false;
        self.image_output_file.clear();
        self.texture = None;
        self.corner_points.clear();
        self.selection = None;
        self.start_button_text = "selection".to_string();
    }
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Default::default()
    }

    #[allow(dead_code)]
    fn load_image<P: AsRef<Path>>(&mut self, path: P) -> Result<(), Box<dyn Error>> {
        let img = image::open(path).unwrap();
        self.max_width = self.max_width.max(img.width());
        self.total_height += img.height();
        self.images.push(img);
        Ok(())
    }

    #[allow(dead_code)]
    fn load_images<P: AsRef<Path>>(&mut self, paths: &[P]) -> Result<(), Box<dyn Error>> {
        for path in paths {
            self.load_image(path).unwrap();
        }
        Ok(())
    }

    #[allow(dead_code)]
    fn process<P: AsRef<Path>>(&mut self, path: P) -> Result<DynamicImage, Box<dyn Error>> {
        if self.images.is_empty() {
            return Err("No images loaded".into());
        }

        let mut output = ImageBuffer::from_fn(self.max_width, self.total_height, |_, _| {
            Rgba([255, 255, 255, 255])
        });

        let mut y_offset = 0;

        for img in &self.images {
            let x_offset = (self.max_width - img.width()) / 2;
            image::imageops::overlay(&mut output, img, x_offset.into(), y_offset);
            y_offset += img.height() as i64;
        }

        let dynamic_image = DynamicImage::ImageRgba8(output);
        println!("{}, {}", dynamic_image.width(), dynamic_image.height());
        dynamic_image
            .save_with_format(&path, ImageFormat::Png)
            .unwrap();

        // let data = String::from("for test");
        // self.str_clipboard.set_contents(data).unwrap();
        // let content = self.str_clipboard.get_contents().unwrap();
        // println!("content: {}", content);

        let img_rgba = dynamic_image.to_rgba8();
        let width = img_rgba.width() as usize;
        let height = img_rgba.height() as usize;
        let bytes = img_rgba.into_raw();
        println!("{width}, {height}, {}", bytes.len());
        // let mut clipboard = Clipboard::new().unwrap();
        // self.image_clipboard
        //     .set_image(arboard::ImageData {
        //         width,
        //         height,
        //         bytes: bytes.into(),
        //     })
        //     .unwrap();
        // let the_string = "testing!";
        // self.image_clipboard.set_text(the_string).unwrap();
        // println!(
        //     "But now the clipboard text should be: text \"{:?}\", image \"{:?}\"",
        //     self.image_clipboard.get_text(),
        //     self.image_clipboard.get_image().unwrap().bytes.len()
        // );

        Ok(dynamic_image)
    }

    fn clear_path(&mut self, path: &Path) {
        if path.exists() {
            println!("Clearing all files in {:?}", path);
            match clear_directory(path) {
                Ok(()) => {
                    println!("Successfully cleared all files in {:?}", path)
                }
                Err(e) => {
                    eprintln!("Failed to clear directory {:?}: {}", path, e)
                }
            }
        } else {
            println!("Directory {:?} does not exist", path);
        }
    }
}

fn clear_directory<P: AsRef<Path>>(dir: P) -> std::io::Result<()> {
    for entry in fs::read_dir(dir.as_ref())? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() {
            fs::remove_file(path)?;
        } else if path.is_dir() {
            fs::remove_dir_all(path)?;
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn capture_screen_area(
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    output_path: &str,
) -> Result<(), String> {
    let output = Command::new("scrot")
        .args(&[
            "-a",
            &format!("{},{},{},{}", x, y, width, height),
            output_path,
        ])
        .output()
        .map_err(|e| format!("执行scrot失败: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        let error = String::from_utf8_lossy(&output.stderr);
        Err(format!("scrot执行出错: {}", error))
    }
}

impl eframe::App for ImageProcessor {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // self.file_prefix += 1;
        // self.enigo
        //     .scroll(self.scroll_num, enigo::Axis::Vertical)
        //     .unwrap();
        // println!("scroll_num: {}, {}", self.scroll_num, self.file_prefix);
        // ctx.request_repaint_after_secs(2.0);
        // return;
        // match get_active_window() {
        //     Ok(window) => {
        //         println!("活动窗口标题: {}", window.title);
        //         println!("应用程序名称: {}", window.app_name);
        //         println!("窗口位置: {:?}", window.position);
        //         // 还可以获取更多窗口信息
        //     }
        //     Err(e) => println!("无法获取活动窗口: {:?}", e),
        // }
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                let is_web = cfg!(target_arch = "wasm32");
                if !is_web {
                    ui.menu_button("File", |ui| {
                        if ui.button("Quit").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    ui.add_space(16.0);
                }
                egui::widgets::global_theme_preference_buttons(ui);
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            // The central panel the region left after adding TopPanel's and SidePanel's
            ui.heading("Image Concatenator");
            ui.separator();

            ui.horizontal(|ui| {
                ui.heading("scroll num: ");
                ui.add(
                    egui::DragValue::new(&mut self.scroll_num)
                        .speed(1)
                        .range(1..=20),
                );
                ui.heading("save path: ");
                ui.add(egui::TextEdit::singleline(&mut self.save_path));
            });
            ui.separator();
            let save_path = self.save_path.clone();
            let path = Path::new(&save_path);
            if !path.exists() {
                match fs::create_dir_all(path) {
                    Ok(()) => println!("Successfully created {:?}", path),
                    Err(e) => eprintln!("Failed to create {:?}: {}", path, e),
                }
            }
            let button_width = 100.;
            let button_height = 50.;
            ui.horizontal(|ui| {
                let mut style: egui::Style = (*ctx.style()).clone();
                style.spacing.interact_size = egui::vec2(button_width, button_height);
                ctx.set_style(style);
                if self.adding_on_progress {
                    self.adding_on_progress = false;
                    if self.file_prefix == 0 {
                        self.reset();
                        self.clear_path(path);
                        if let Ok(selection) = ScreenSelection::from_slop() {
                            self.selection = Some(selection);
                        }
                    }
                    let mut path_buf = path.to_path_buf();
                    path_buf.push(format!("{}.png", self.file_prefix));
                    let path_str = path_buf.to_str().unwrap();
                    if let Some(selection) = &self.selection {
                        self.start_button_text = "step".to_string();
                        if let Ok(_) = selection.capture_screenshot(path_str) {
                            self.enigo
                                .move_mouse(selection.left_x(), selection.top_y(), Abs)
                                .unwrap();
                            thread::sleep(Duration::from_millis(10));
                            self.enigo
                                .scroll(self.scroll_num, enigo::Axis::Vertical)
                                .unwrap();
                            //  Should sleep!
                            thread::sleep(Duration::from_millis(10));
                            self.enigo
                                .move_mouse(
                                    self.start_checkbox_pos.0,
                                    self.start_checkbox_pos.1,
                                    Abs,
                                )
                                .unwrap();
                            self.image_log = format!("Current: {}", &path_str);
                            self.paths.push(path_str.to_string());
                            self.file_prefix += 1;
                        } else {
                            self.adding_on_progress = false;
                            self.image_log = "No screenshot".to_string();
                        }
                    } else {
                        self.adding_on_progress = false;
                        self.image_log = "No selection".to_string();
                    }
                }
                let rich_text = egui::RichText::new(&self.start_button_text)
                    .strong()
                    .font(egui::FontId::monospace(26.));
                if ui
                    .checkbox(&mut self.adding_on_progress, rich_text)
                    .clicked()
                {
                    self.start_checkbox_pos = self.device_state.get_mouse().coords;
                }

                ui.separator();
                let rich_text = egui::RichText::new("save".to_string())
                    .strong()
                    .font(egui::FontId::monospace(16.));
                let button =
                    egui::Button::new(rich_text).min_size(egui::vec2(button_width, button_height));
                if ui.add(button).clicked() {
                    let mut path_buf = path.to_path_buf();
                    path_buf.push(&self.file_name);
                    // let paths = self.paths.clone();
                    // self.load_images(&paths).unwrap();
                    // No remove find_overlapping_region_ncc.
                    // if self.process(&path_buf).is_ok() {
                    //     self.image_output_file = path_buf;
                    //     self.image_log = format!("Save to: {:?}", self.image_output_file);
                    //     self.file_prefix = 0;
                    // }
                    if self.concatenate_images(&path_buf).is_ok() {
                        self.reset();
                        self.image_output_file = path_buf;
                        self.image_log = format!("Save to: {:?}", self.image_output_file);
                    }
                }
                ui.separator();
                let rich_text = egui::RichText::new("clear".to_string())
                    .strong()
                    .font(egui::FontId::monospace(16.));
                let button =
                    egui::Button::new(rich_text).min_size(egui::vec2(button_width, button_height));
                if ui.add(button).clicked() {
                    self.reset();
                    self.clear_path(path);
                }
            });
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new("📋")
                            .min_size(egui::vec2(button_width * 0.5, button_height * 0.5)),
                    )
                    .clicked()
                {
                    ui.ctx().copy_text(self.image_log.clone());
                }
                ui.label(format!("{}", &self.image_log));
            });
            ui.separator();
            let image_path = self.image_output_file.as_path().to_path_buf();
            let _ = self.load_image_from_path(&image_path, ctx);
            if let Some(texture) = &self.texture {
                egui::Image::new(texture).shrink_to_fit().ui(ui);
            } else {
                ui.label("Failed to load image");
            }

            ui.separator();

            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                powered_by_egui_and_eframe(ui);
                egui::warn_if_debug_build(ui);
            });
        });
    }
}

fn powered_by_egui_and_eframe(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.label("Powered by ");
        ui.hyperlink_to("egui", "https://github.com/emilk/egui");
        ui.label(" and ");
        ui.hyperlink_to(
            "eframe",
            "https://github.com/emilk/egui/tree/master/crates/eframe",
        );
        ui.label(".");
    });
}
