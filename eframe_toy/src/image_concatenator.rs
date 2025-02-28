use arboard::Clipboard;
use copypasta::{x11_clipboard::X11ClipboardContext, ClipboardContext};
use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};
use std::{error::Error, fs, path::Path, process::Command};

#[allow(dead_code)]
pub struct ImageProcessor {
    images: Vec<DynamicImage>,
    max_width: u32,
    total_height: u32,
    save_path: String,
    output_file: String,
    file_prefix: usize,
    paths: Vec<String>,
    image_log: String,
    adding_on_progress: bool,
    str_clipboard: X11ClipboardContext,
    image_clipboard: Clipboard,
}
impl Default for ImageProcessor {
    fn default() -> Self {
        Self {
            images: Vec::new(),
            max_width: 0,
            total_height: 0,
            save_path: "/tmp/image_dir/".to_string(),
            output_file: String::from("output.png"),
            file_prefix: 0,
            paths: vec![],
            image_log: String::new(),
            adding_on_progress: false,
            str_clipboard: ClipboardContext::new().unwrap(),
            image_clipboard: Clipboard::new().unwrap(),
        }
    }
}

impl ImageProcessor {
    pub fn reset(&mut self) {
        self.images.clear();
        self.max_width = 0;
        self.total_height = 0;
        self.file_prefix = 0;
        self.paths.clear();
        self.image_log.clear();
        self.adding_on_progress = false;
    }
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Default::default()
    }

    fn load_image<P: AsRef<Path>>(&mut self, path: P) -> Result<(), Box<dyn Error>> {
        let img = image::open(path).unwrap();
        self.max_width = self.max_width.max(img.width());
        self.total_height += img.height();
        self.images.push(img);
        Ok(())
    }

    fn load_images<P: AsRef<Path>>(&mut self, paths: &[P]) -> Result<(), Box<dyn Error>> {
        for path in paths {
            self.load_image(path).unwrap();
        }
        Ok(())
    }

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
        self.image_clipboard
            .set_image(arboard::ImageData {
                width,
                height,
                bytes: bytes.into(),
            })
            .unwrap();
        // let the_string = "testing!";
        // self.image_clipboard.set_text(the_string).unwrap();
        println!(
            "But now the clipboard text should be: text \"{:?}\", image \"{:?}\"",
            self.image_clipboard.get_text(),
            self.image_clipboard.get_image().unwrap().bytes.len()
        );

        Ok(dynamic_image)
    }

    fn clear_path(&mut self, path: &Path) {
        self.reset();
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
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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
            ui.horizontal(|ui| {
                let button_width = 100.;
                let button_height = 50.;
                let mut style: egui::Style = (*ctx.style()).clone();
                style.spacing.interact_size = egui::vec2(button_width, button_height);
                ctx.set_style(style);
                if self.adding_on_progress {
                    self.adding_on_progress = false;
                    if self.file_prefix == 0 {
                        self.clear_path(path);
                    }
                    let mut path_buf = path.to_path_buf();
                    path_buf.push(format!("{}.png", self.file_prefix));
                    let path_str = path_buf.to_str().unwrap();
                    let status = Command::new("scrot")
                        .arg("-s")
                        .arg(&path_str)
                        .status()
                        .expect("Failed to execute scrot command");
                    if status.success() {
                        self.image_log = format!("Current: {}", &path_str);
                        self.paths.push(path_str.to_string());
                        self.file_prefix += 1;
                    } else {
                        self.adding_on_progress = false;
                        self.image_log = "escape screen shot".to_string();
                    }
                }
                let rich_text = egui::RichText::new("start".to_string())
                    .strong()
                    .font(egui::FontId::monospace(26.));
                ui.checkbox(&mut self.adding_on_progress, rich_text);

                ui.separator();
                let rich_text = egui::RichText::new("save".to_string())
                    .strong()
                    .font(egui::FontId::monospace(16.));
                let button =
                    egui::Button::new(rich_text).min_size(egui::vec2(button_width, button_height));
                if ui.add(button).clicked() {
                    let paths = self.paths.clone();
                    self.load_images(&paths).unwrap();
                    let mut path_buf = path.to_path_buf();
                    path_buf.push(&self.output_file);
                    if self.process(path_buf.to_str().unwrap()).is_ok() {
                        self.image_log = format!("Save to: {}", path_buf.to_str().unwrap());
                        self.file_prefix = 0;
                    }
                }
                ui.separator();
                let rich_text = egui::RichText::new("clear".to_string())
                    .strong()
                    .font(egui::FontId::monospace(16.));
                let button =
                    egui::Button::new(rich_text).min_size(egui::vec2(button_width, button_height));
                if ui.add(button).clicked() {
                    self.clear_path(path);
                }
            });
            ui.label(format!("Log: {}", &self.image_log));
            ui.separator();
            ui.heading("Output:");

            ui.separator();

            // ui.add(egui::github_link_file!(
            //     "https://github.com/emilk/eframe_template/blob/main/",
            //     "Source code."
            // ));
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
