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
    current_path_string: String,
    selected_item: Option<PathBuf>,
    files: Vec<FileItem>,
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
            current_path_string: current_path.to_string_lossy().to_string(),
            current_path,
            selected_item: None,
            files: Vec::new(),
        };
        explorer.refresh_files();
        explorer
    }
}
impl MyApp {
    fn refresh_files(&mut self) {
        self.current_path_string = self.current_path.to_string_lossy().to_string();
        self.files.clear();
        if let Ok(entries) = std::fs::read_dir(&self.current_path) {
            for entry in entries.flatten() {
                if let Ok(metadata) = entry.metadata() {
                    self.files.push(FileItem {
                        name: entry.file_name().to_string_lossy().to_string(),
                        path: entry.path(),
                        is_dir: metadata.is_dir(),
                        metadata,
                    });
                }
            }
        }
        self.files.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.cmp(&b.name),
        });
    }

    fn render_folder_tree(&mut self, ui: &mut egui::Ui) {
        if let Ok(entries) = std::fs::read_dir(&self.current_path) {
            ui.vertical(|ui| {
                egui::ScrollArea::vertical()
                    .id_salt("main_list")
                    .show(ui, |ui| {
                        for entry in entries.flatten() {
                            if let Ok(metadata) = entry.metadata() {
                                let path = entry.path();
                                let is_selected = self.current_path == path;
                                let folder_name = path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .to_string();
                                let is_dir = metadata.is_dir();
                                let icon = if is_dir { "📁" } else { "📄" };
                                let response = ui.selectable_label(
                                    is_selected,
                                    format!("{} {}", icon, folder_name),
                                );
                                if response.clicked() {
                                    if is_dir {
                                        self.current_path = path.to_path_buf();
                                        self.refresh_files();
                                    } else {
                                        self.current_path_string =
                                            path.to_path_buf().to_string_lossy().to_string();
                                    }
                                }
                            }
                        }
                    });
            });
        }
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
                    }
                }
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.current_path_string)
                        .desired_width(ui.available_width()),
                );
                if response.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    if let Ok(new_path) = PathBuf::from(&self.current_path_string).canonicalize() {
                        self.current_path = new_path;
                        self.refresh_files();
                    } else {
                        // Keep unchanged
                        self.current_path_string = self.current_path.to_string_lossy().to_string();
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
