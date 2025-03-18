use arboard::Clipboard;
use device_query::{DeviceQuery, DeviceState};
use enigo::Coordinate::Abs;
use enigo::{Enigo, Mouse, Settings};
use image::DynamicImage;
use std::error::Error;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;
use std::{fs, path::Path};

use crate::{correlation_stitcher, direct_stitcher, ScreenSelection};

use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize, Debug)]
pub struct UserInfo {
    pub save_path: String,
    pub output_file_name: String,
    pub scroll_num: i32,
    pub pixels_per_scroll: i32,
}
impl Default for UserInfo {
    fn default() -> Self {
        Self {
            save_path: "/tmp/image_dir/".to_string(),
            output_file_name: String::from("output.png"),
            scroll_num: 5,
            pixels_per_scroll: 120,
        }
    }
}
pub struct ImageProcessor {
    pub last_image: Option<DynamicImage>,
    pub file_prefix: usize,
    pub paths: Vec<String>,
    pub image_log: String,
    pub adding_on_progress: bool,
    pub image_clipboard: Clipboard,
    pub text: String,
    pub image_output_file: PathBuf,
    pub textures: Vec<egui::TextureHandle>,
    pub device_state: DeviceState,
    pub enigo: Enigo,
    pub start_checkbox_pos: (i32, i32),
    pub selection: Option<ScreenSelection>,
    pub start_button_text: String,
    pub scroll_delta_x: i32,
    pub scroll_delta_y: i32,
    pub user_info: UserInfo,
}
impl Default for ImageProcessor {
    fn default() -> Self {
        Self {
            last_image: None,
            file_prefix: 0,
            paths: vec![],
            image_log: String::new(),
            adding_on_progress: false,
            image_clipboard: Clipboard::new().unwrap(),
            text: String::new(),
            image_output_file: PathBuf::new(),
            textures: Vec::new(),
            device_state: DeviceState::new(),
            enigo: Enigo::new(&Settings::default()).unwrap(),
            start_checkbox_pos: (0, 0),
            selection: None,
            start_button_text: "selection".to_string(),
            scroll_delta_x: 0,
            scroll_delta_y: 0,
            user_info: UserInfo::default(),
        }
    }
}

impl ImageProcessor {
    pub fn open_folder(&self, output_path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let _ = Command::new("nautilus")
            .arg(output_path)
            .spawn()
            .expect("failed to open folder");
        Ok(())
    }

    fn concatenate_images(&mut self, output_path: &PathBuf) -> Result<(), Box<dyn Error>> {
        let mut image_paths = Vec::new();
        for dir in &self.paths {
            image_paths.push(PathBuf::from(dir));
        }

        let use_direct = false;
        let y_offset = self.user_info.scroll_num * self.user_info.pixels_per_scroll;
        let similarity_threshold = 0.9;
        let use_fixed_offset = false;
        if use_direct {
            let offset_range = 0.2; //(±20%）
            let stitcher = direct_stitcher::ScrollStitcher::new(y_offset as u32, offset_range);
            stitcher.process_directory(image_paths, output_path.to_path_buf())?;
        } else {
            let stitcher = correlation_stitcher::ScrollStitcher::new(
                y_offset as u32,
                similarity_threshold,
                use_fixed_offset,
            );
            stitcher.process_directory(image_paths, output_path.to_path_buf())?;
        }

        Ok(())
    }

    pub fn reset(&mut self) {
        self.last_image = None;
        self.file_prefix = 0;
        self.paths.clear();
        self.image_log.clear();
        self.adding_on_progress = false;
        self.image_output_file.clear();
        self.textures.clear();
        self.selection = None;
        self.start_button_text = "selection".to_string();
    }
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        if let Some(storage) = cc.storage {
            let user_info =
                eframe::get_value::<UserInfo>(storage, eframe::APP_KEY).unwrap_or_default();
            return Self {
                user_info,
                ..Default::default()
            };
        }
        Default::default()
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

impl eframe::App for ImageProcessor {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.user_info);
    }
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
                if ui.button("calibration").clicked() {
                    if let Ok((dx, dy)) = self.verify_scroll_pixel() {
                        self.scroll_delta_x = dx;
                        self.scroll_delta_y = dy;
                    }
                }
                ui.label(format!(
                    "scroll pixel dx: {}, dy: {}",
                    self.scroll_delta_x, self.scroll_delta_y
                ));
                ui.separator();
                ui.heading("pixels_per_scroll: ");
                ui.add(
                    egui::DragValue::new(&mut self.user_info.pixels_per_scroll)
                        .speed(1)
                        .range(1..=200),
                );
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.heading("scroll num: ");
                ui.add(
                    egui::DragValue::new(&mut self.user_info.scroll_num)
                        .speed(1)
                        .range(0..=20),
                );
                ui.heading("save path: ");
                ui.add(egui::TextEdit::singleline(&mut self.user_info.save_path));
            });
            ui.separator();
            let save_path = self.user_info.save_path.clone();
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
                            let current_image = image::open(&path_buf).unwrap();
                            let mut same_image = false;
                            if let Some(prev_image) = &self.last_image {
                                if *prev_image == current_image {
                                    same_image = true;
                                }
                            }
                            if same_image {
                                self.image_log = "Skip same screenshot! ".to_string();
                            } else {
                                self.last_image = Some(current_image);
                                self.enigo
                                    .move_mouse(selection.left_x(), selection.top_y(), Abs)
                                    .unwrap();
                                thread::sleep(Duration::from_millis(10));
                                self.enigo
                                    .scroll(self.user_info.scroll_num, enigo::Axis::Vertical)
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
                            }
                        } else {
                            self.image_log = "No screenshot!".to_string();
                        }
                    } else {
                        self.image_log = "No selection!".to_string();
                    }
                }
                let rich_text = egui::RichText::new(&self.start_button_text)
                    .strong()
                    .font(egui::FontId::monospace(26.));
                if ui
                    .add(
                        egui::Button::new(rich_text)
                            .min_size(egui::vec2(button_width, button_height)),
                    )
                    .clicked()
                {
                    self.adding_on_progress = true;
                    self.start_checkbox_pos = self.device_state.get_mouse().coords;
                }

                ui.separator();
                let rich_text = egui::RichText::new("save".to_string())
                    .strong()
                    .font(egui::FontId::monospace(16.));
                let button =
                    egui::Button::new(rich_text).min_size(egui::vec2(button_width, button_height));
                if ui.add(button).clicked() {
                    let start = std::time::Instant::now();
                    let mut path_buf = path.to_path_buf();
                    path_buf.push(&self.user_info.output_file_name);
                    if self.concatenate_images(&path_buf).is_ok() {
                        self.reset();
                        self.image_output_file = path_buf;
                        let time_cost = start.elapsed().as_secs_f32();
                        self.image_log = format!(
                            "Save to: {:?}, time cost: {:.3} seconds",
                            self.image_output_file, time_cost
                        );
                        let image_path = self.image_output_file.as_path().to_path_buf();
                        let _ = self.load_image_from_path(&image_path, ctx);
                    }
                }
                ui.separator();
                if ui
                    .add(
                        egui::Button::new("Foler 📋")
                            .min_size(egui::vec2(button_width, button_height)),
                    )
                    .clicked()
                {
                    if self.image_output_file.is_file()
                        && self
                            .open_folder(self.image_output_file.to_str().unwrap())
                            .is_ok()
                    {
                        self.image_log = "Open image folder".to_string();
                    }
                    // ui.ctx().copy_text(self.image_log.clone());
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
            ui.label(format!("{}", &self.image_log));
            ui.separator();
            self.scroll_display(ui);

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
