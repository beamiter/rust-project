use std::path::PathBuf;

use eframe::egui;
use egui::{FontFamily, FontId, TextStyle};
use std::collections::BTreeMap;

fn configure_text_styles(ctx: &egui::Context) {
    use FontFamily::{Monospace, Proportional};

    let text_styles: BTreeMap<TextStyle, FontId> = [
        (TextStyle::Small, FontId::new(12.0, Proportional)),
        (TextStyle::Body, FontId::new(16.0, Proportional)),
        (TextStyle::Monospace, FontId::new(16.0, Monospace)),
        (TextStyle::Button, FontId::new(16.0, Proportional)),
        (TextStyle::Heading, FontId::new(25.0, Proportional)),
    ]
    .into();
    ctx.all_styles_mut(move |style| style.text_styles = text_styles.clone());
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([900.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "My egui App",
        options,
        Box::new(|cc| {
            configure_text_styles(&cc.egui_ctx);
            Ok(Box::<MyApp>::default())
        }),
    )
}

#[allow(dead_code)]
struct MyApp {
    current_path: PathBuf,
    current_files: Vec<FileItem>,
    edit_path_string: String,
    hovered_path: Option<PathBuf>,
    hovered_files: Vec<FileItem>,
}
#[allow(dead_code)]
struct FileItem {
    name: String,
    path: PathBuf,
    metadata: std::fs::Metadata,
    is_dir: bool,
}

impl Default for MyApp {
    fn default() -> Self {
        let current_path = std::env::current_dir().unwrap_or_default();
        let mut explorer = Self {
            edit_path_string: current_path.to_string_lossy().to_string(),
            current_path,
            hovered_path: None,
            current_files: Vec::new(),
            hovered_files: Vec::new(),
        };
        explorer.refresh_files();
        explorer
    }
}
impl MyApp {
    fn load_directory(&self, path: &PathBuf) -> Vec<FileItem> {
        std::fs::read_dir(path)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|entry| {
                        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                        FileItem {
                            name: entry.file_name().to_string_lossy().to_string(),
                            path: entry.path(),
                            metadata: entry.metadata().unwrap(),
                            is_dir,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
    fn refresh_files(&mut self) {
        self.edit_path_string = self.current_path.to_string_lossy().to_string();
        self.current_files = self.load_directory(&self.current_path);
        self.current_files
            .sort_by(|a, b| match (a.is_dir, b.is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.name.cmp(&b.name),
            });
    }

    fn render_folder_tree(&mut self, ui: &mut egui::Ui) {
        let available_height = ui.available_height();
        ui.horizontal(|ui| {
            ui.set_max_height(available_height);
            ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                // 第一个ScrollArea
                egui::ScrollArea::vertical()
                    .id_salt("main_list")
                    .show(ui, |ui| {
                        for i in 0..self.current_files.len() {
                            let fs = &self.current_files[i];
                            let path = &fs.path;
                            let path_string = path.to_path_buf().to_string_lossy().to_string();
                            let is_selected = self.current_path == *path;
                            let folder_name = &fs.name;
                            let is_dir = fs.is_dir;
                            let icon = if is_dir { "📁" } else { "📄" };
                            let response = ui
                                .selectable_label(is_selected, format!("{} {}", icon, folder_name));
                            if response.clicked() {
                                if is_dir {
                                    self.current_path = path.to_path_buf();
                                    self.refresh_files();
                                    break;
                                } else {
                                    self.edit_path_string = path_string.clone();
                                }
                            }
                            if response.hovered() {
                                response.on_hover_text(path_string);
                                if self.hovered_path.as_ref() != Some(path) {
                                    self.hovered_path = Some(path.clone());
                                }
                                if is_dir {
                                    self.hovered_files = self.load_directory(path);
                                } else {
                                    self.hovered_files.clear();
                                }
                            }
                        }
                    });
            });
            ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                // 第二个ScrollArea
                egui::ScrollArea::vertical()
                    .id_salt("side_list")
                    .show(ui, |ui| {
                        if !self.hovered_files.is_empty() {
                            for i in 0..self.hovered_files.len() {
                                let fs = &self.hovered_files[i];
                                let path = &fs.path;
                                let path_string = path.to_path_buf().to_string_lossy().to_string();
                                let is_selected = self.hovered_path == Some(path.to_path_buf());
                                let folder_name = &fs.name;
                                let is_dir = fs.is_dir;
                                let icon = if is_dir { "📁" } else { "📄" };
                                let response = ui.selectable_label(
                                    is_selected,
                                    format!("{} {}", icon, folder_name),
                                );
                                if response.clicked() {
                                    if is_dir {
                                        self.current_path = path.to_path_buf();
                                        self.refresh_files();
                                        self.hovered_path = None;
                                        self.hovered_files.clear();
                                        break;
                                    } else {
                                        self.edit_path_string = path_string.clone();
                                    }
                                }
                            }
                        }
                    });
            });
        });
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("<").clicked() {
                    if let Some(parent) = self.current_path.parent() {
                        self.current_path = parent.to_path_buf();
                        self.refresh_files();
                        self.hovered_path = None;
                        self.hovered_files.clear();
                    }
                }
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.edit_path_string)
                        .desired_width(ui.available_width()),
                );
                if response.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    if let Ok(new_path) = PathBuf::from(&self.edit_path_string).canonicalize() {
                        self.current_path = new_path;
                        self.refresh_files();
                    } else {
                        // Keep unchanged
                        self.edit_path_string = self.current_path.to_string_lossy().to_string();
                    }
                }
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Folders");
            self.render_folder_tree(ui);
        });
    }
}
