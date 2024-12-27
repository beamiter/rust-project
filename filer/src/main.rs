use std::{collections::HashMap, path::PathBuf};

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

struct MyApp {
    edit_path: PathBuf,
    edit_path_string: String,
    selected_path_chain: Vec<Option<PathBuf>>,
    sub_dirs_dict: HashMap<PathBuf, Vec<FileItem>>,
}
#[allow(dead_code)]
#[derive(Debug, Clone)]
struct FileItem {
    name: String,
    path: PathBuf,
    metadata: std::fs::Metadata,
    is_dir: bool,
}

impl Default for MyApp {
    fn default() -> Self {
        let current_path = std::env::current_dir().unwrap_or_default();
        let mut res = Self {
            edit_path_string: current_path.to_string_lossy().to_string(),
            selected_path_chain: Vec::with_capacity(Self::MAX_DEPTH),
            sub_dirs_dict: HashMap::new(),
            edit_path: current_path.clone(),
        };
        res.selected_path_chain.push(Some(current_path.clone()));
        res.refresh_sub_dirs(&current_path);
        res
    }
}
impl MyApp {
    const MAX_DEPTH: usize = 8;
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

    fn refresh_sub_dirs(&mut self, path: &PathBuf) {
        let files = self.load_directory(path);
        self.sub_dirs_dict.insert(path.to_path_buf(), files);
    }

    fn get_sub_dirs(&self, node: &PathBuf) -> Vec<FileItem> {
        match self.sub_dirs_dict.get(node) {
            Some(ref val) => {
                return val.to_vec();
            }
            None => {
                return Vec::new();
            }
        }
    }

    fn is_in_path_chain(&self, path: &PathBuf) -> bool {
        for tmp in &self.selected_path_chain {
            if let Some(ref val) = tmp {
                if path == val {
                    return true;
                }
            } else {
                break;
            }
        }
        false
    }

    fn render_folder_tree(&mut self, ui: &mut egui::Ui) {
        let available_height = ui.available_height();
        ui.horizontal(|ui| {
            ui.set_max_height(available_height);
            for idx in 0..Self::MAX_DEPTH {
                if idx >= self.selected_path_chain.len() {
                    break;
                }
                let node_i = self.selected_path_chain[idx].clone();
                if node_i.is_none() {
                    break;
                }
                let node_i = node_i.unwrap();
                let sub_dirs_i = self.get_sub_dirs(&node_i);
                if sub_dirs_i.is_empty() {
                    break;
                }
                ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt(format!("level_{}", idx))
                        .show(ui, |ui| {
                            for i in 0..sub_dirs_i.len() {
                                let fs = &sub_dirs_i[i];
                                let path = &fs.path;
                                let path_string = path.to_path_buf().to_string_lossy().to_string();
                                let is_selected = self.is_in_path_chain(path);
                                let folder_name = &fs.name;
                                let is_dir = fs.is_dir;
                                let icon = if is_dir { "📁" } else { "  " };
                                let response = ui.selectable_label(
                                    is_selected,
                                    format!("{} {}", icon, folder_name),
                                );
                                if response.clicked() {
                                    if is_dir {
                                        self.reset_root_with_path(path);
                                        break;
                                    } else {
                                        self.edit_path_string = path_string.clone();
                                    }
                                }
                                if response.hovered() {
                                    response.on_hover_text(path_string);
                                    let next_idx = idx + 1;
                                    if next_idx >= self.selected_path_chain.len() {
                                        self.selected_path_chain.push(Some(path.to_path_buf()));
                                    } else {
                                        self.selected_path_chain[next_idx] =
                                            Some(path.to_path_buf());
                                        self.selected_path_chain.truncate(next_idx + 1);
                                    }
                                    if is_dir {
                                        self.refresh_sub_dirs(path);
                                    }
                                }
                            }
                        });
                });
            }
        });
    }

    fn reset_root_with_path(&mut self, path: &PathBuf) {
        self.selected_path_chain.clear();
        self.selected_path_chain.push(Some(path.clone()));
        self.edit_path = path.clone();
        self.edit_path_string = self.edit_path.to_string_lossy().to_string();
        self.refresh_sub_dirs(&path);
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("<").clicked() {
                    if let Some(parent) = self.edit_path.parent() {
                        self.reset_root_with_path(&parent.to_path_buf());
                    }
                }
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.edit_path_string)
                        .desired_width(ui.available_width()),
                );
                if response.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    if let Ok(path) = PathBuf::from(&self.edit_path_string).canonicalize() {
                        self.reset_root_with_path(&path);
                    } else {
                        // Keep unchanged
                        self.edit_path_string = self.edit_path.to_string_lossy().to_string();
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
