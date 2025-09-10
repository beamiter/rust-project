use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use eframe::egui;
use egui::{FontFamily, FontId, TextStyle};

pub fn configure_text_styles(ctx: &egui::Context) {
    // 更通用的设置方式：clone 当前 style，修改后 set_style
    let mut style = (*ctx.style()).clone();

    use FontFamily::{Monospace, Proportional};
    let text_styles: BTreeMap<TextStyle, FontId> = [
        (TextStyle::Small, FontId::new(12.0, Proportional)),
        (TextStyle::Body, FontId::new(16.0, Proportional)),
        (TextStyle::Monospace, FontId::new(14.0, Monospace)),
        (TextStyle::Button, FontId::new(16.0, Proportional)),
        (TextStyle::Heading, FontId::new(24.0, Proportional)),
    ]
    .into();

    style.text_styles = text_styles;

    // 可选：微调整体可视化细节
    // let mut visuals = egui::Visuals::dark();
    // visuals.window_rounding = 6.0.into();
    // visuals.widgets.inactive.rounding = 4.0.into();
    // style.visuals = visuals;

    ctx.set_style(style);
}

#[derive(Debug, Clone)]
struct FileItem {
    name: String,
    path: PathBuf,
    is_dir: bool,
    is_hidden: bool,
}

impl FileItem {
    fn from_entry(entry: &std::fs::DirEntry) -> Option<Self> {
        let file_name_os = entry.file_name();
        let name = file_name_os.to_string_lossy().to_string();

        // file_type 比 metadata 更轻量
        let ty = entry.file_type().ok()?;
        let is_dir = ty.is_dir();

        // 简单隐含规则：以 '.' 开头
        let is_hidden = name.starts_with('.');

        Some(Self {
            name,
            path: entry.path(),
            is_dir,
            is_hidden,
        })
    }
}

pub struct Filer {
    // 当前路径
    edit_path: PathBuf,
    edit_path_string: String,

    // 列浏览的路径链：chain[0] = 根；chain[i] 的下一列显示它的子项
    selected_path_chain: Vec<PathBuf>,

    // 目录缓存：每个目录对应的子项（文件+文件夹）
    cache: HashMap<PathBuf, Vec<FileItem>>,

    // UI 选项
    show_hidden: bool,
    dirs_only: bool,
    preview_on_hover: bool,

    // 状态与错误
    error_msg: Option<String>,
}

impl Default for Filer {
    fn default() -> Self {
        let current_path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut me = Self {
            edit_path_string: current_path.to_string_lossy().to_string(),
            selected_path_chain: Vec::with_capacity(Self::MAX_DEPTH),
            cache: HashMap::new(),
            edit_path: current_path.clone(),

            show_hidden: false,
            dirs_only: false,
            preview_on_hover: true,

            error_msg: None,
        };

        me.selected_path_chain.push(current_path.clone());
        me.ensure_cached(&current_path);
        me
    }
}

impl Filer {
    const MAX_DEPTH: usize = 8;

    fn ensure_cached(&mut self, path: &Path) {
        if self.cache.contains_key(path) {
            return;
        }
        let list = self.load_directory(path);
        self.cache.insert(path.to_path_buf(), list);
    }

    fn load_directory(&self, path: &Path) -> Vec<FileItem> {
        let mut items: Vec<FileItem> = match std::fs::read_dir(path) {
            Ok(entries) => entries
                .filter_map(Result::ok)
                .filter_map(|entry| FileItem::from_entry(&entry))
                .collect(),
            Err(e) => {
                eprintln!("read_dir error for {}: {}", path.display(), e);
                Vec::new()
            }
        };

        // 排序：目录优先 + 名称（大小写不敏感）
        items.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        });

        items
    }

    fn get_children(&mut self, parent: &Path) -> &[FileItem] {
        self.ensure_cached(parent);
        self.cache.get(parent).map(|v| v.as_slice()).unwrap_or(&[])
    }

    fn in_chain_at_level_is(&self, level: usize, path: &Path) -> bool {
        if level + 1 < self.selected_path_chain.len() {
            &self.selected_path_chain[level + 1] == path
        } else {
            false
        }
    }

    fn reset_root_with_path(&mut self, path: &Path) {
        self.selected_path_chain.clear();
        self.selected_path_chain.push(path.to_path_buf());
        self.edit_path = path.to_path_buf();
        self.edit_path_string = self.edit_path.to_string_lossy().to_string();
        self.ensure_cached(path);
    }

    fn go_parent(&mut self) {
        if let Some(parent) = self.edit_path.parent() {
            let parent_path = parent.to_path_buf(); // 克隆路径
            self.reset_root_with_path(&parent_path);
        }
    }

    fn go_home(&mut self) {
        if let Some(home) = dirs::home_dir() {
            self.reset_root_with_path(&home);
        }
    }

    fn commit_path_input(&mut self) {
        let input = self.edit_path_string.trim();
        if input.is_empty() {
            return;
        }
        match PathBuf::from(input).canonicalize() {
            Ok(path) => {
                self.reset_root_with_path(&path);
                self.error_msg = None;
            }
            Err(e) => {
                // 保持不变，并提示错误
                self.edit_path_string = self.edit_path.to_string_lossy().to_string();
                self.error_msg = Some(format!("Invalid path: {}", e));
            }
        }
    }

    fn render_top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui
                .button("Home")
                .on_hover_text("Go to home directory")
                .clicked()
            {
                self.go_home();
            }
            if ui
                .button("Up")
                .on_hover_text("Go to parent directory")
                .clicked()
            {
                self.go_parent();
            }
            if ui
                .button("Refresh")
                .on_hover_text("Refresh current directory")
                .clicked()
            {
                // 清理并重新缓存当前目录
                self.cache.remove(&self.edit_path);
                self.ensure_cached(&&self.edit_path.to_path_buf());
            }

            ui.separator();

            let response = ui.add(
                egui::TextEdit::singleline(&mut self.edit_path_string)
                    .font(TextStyle::Monospace) // 路径用等宽字体更易读
                    .desired_width(ui.available_width() - 64.0),
            );

            // 按 Enter 提交
            let pressed_enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            if pressed_enter || ui.button("Go").clicked() {
                self.commit_path_input();
            } else if response.lost_focus() {
                // 失焦不自动提交，避免误跳转
            }
        });

        ui.horizontal(|ui| {
            ui.checkbox(&mut self.show_hidden, "Show hidden");
            ui.checkbox(&mut self.dirs_only, "Dirs only");
            ui.checkbox(&mut self.preview_on_hover, "Hover preview");
        });

        if let Some(err) = &self.error_msg {
            ui.colored_label(egui::Color32::RED, err);
        }
    }

    fn render_folder_tree(&mut self, ui: &mut egui::Ui) {
        let available_height = ui.available_height();

        ui.horizontal(|ui| {
            ui.set_max_height(available_height);

            // 遍历层级列
            for level in 0..Self::MAX_DEPTH {
                if level >= self.selected_path_chain.len() {
                    break;
                }

                let node = self.selected_path_chain[level].clone();

                // 预先提取需要的字段值
                let dirs_only = self.dirs_only;
                let show_hidden = self.show_hidden;
                let preview_on_hover = self.preview_on_hover;

                // 获取子项并立即克隆，避免持续借用
                let children_full = self.get_children(&node);
                let children_owned: Vec<FileItem> = children_full
                    .iter()
                    .filter(|it| (!dirs_only || it.is_dir) && (show_hidden || !it.is_hidden))
                    .cloned() // 克隆每个 FileItem
                    .collect();

                if children_owned.is_empty() {
                    break;
                }

                ui.vertical(|ui| {
                    // 关键：不同列用不同 id，同时加入路径，保持滚动稳定
                    egui::ScrollArea::vertical()
                        .id_salt(format!("level_{}_{}", level, node.display()))
                        .show(ui, |ui| {
                            for item in &children_owned {
                                // 现在使用引用遍历拥有的数据
                                let icon = if item.is_dir { "📁" } else { "📄" };
                                // 高亮：该列中被选定的"下一级路径"
                                let selected_here = self.in_chain_at_level_is(level, &item.path);

                                let response = ui.selectable_label(
                                    selected_here,
                                    format!("{} {}", icon, item.name),
                                );

                                // 点击：目录 -> 切换根；文件 -> 仅更新输入框与当前路径
                                if response.double_clicked() || response.clicked() {
                                    if item.is_dir {
                                        self.reset_root_with_path(&item.path);
                                    } else {
                                        self.edit_path = item.path.clone();
                                        self.edit_path_string =
                                            self.edit_path.to_string_lossy().to_string();
                                        if self.selected_path_chain.len() > level + 1 {
                                            self.selected_path_chain.truncate(level + 1);
                                        }
                                    }
                                }

                                // 悬停预览：仅目录才预览，并且只影响下一列
                                if preview_on_hover && item.is_dir && response.hovered() {
                                    self.ensure_cached(&item.path);

                                    if self.selected_path_chain.len() == level + 1 {
                                        self.selected_path_chain.push(item.path.clone());
                                    } else {
                                        self.selected_path_chain[level + 1] = item.path.clone();
                                        self.selected_path_chain.truncate(level + 2);
                                    }
                                }

                                // 悬停提示完整路径
                                let hover_text = item.path.display().to_string();
                                if response.hovered() {
                                    response.clone().on_hover_text(hover_text);
                                }

                                // 右键菜单
                                response.context_menu(|ui| {
                                    if ui.button("Copy full path").clicked() {
                                        ui.ctx().copy_text(item.path.display().to_string());
                                        ui.close_menu();
                                    }
                                    if ui.button("Open in file manager").clicked() {
                                        let path = item.path.clone();
                                        #[cfg(target_os = "linux")]
                                        {
                                            let _ = std::process::Command::new("xdg-open")
                                                .arg(&path)
                                                .spawn();
                                        }
                                        #[cfg(target_os = "windows")]
                                        {
                                            let _ = std::process::Command::new("explorer")
                                                .arg(&path)
                                                .spawn();
                                        }
                                        #[cfg(target_os = "macos")]
                                        {
                                            let _ = std::process::Command::new("open")
                                                .arg(&path)
                                                .spawn();
                                        }
                                        ui.close_menu();
                                    }
                                });
                            }
                        });
                });
            }
        });
    }
}

impl eframe::App for Filer {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 样式可在外部 main 中调用 configure_text_styles(ctx)
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            self.render_top_bar(ui);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Folders");
            ui.separator();
            self.render_folder_tree(ui);
        });

        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("Current: {}", self.edit_path.display()));

                // 统计当前列元素数
                let count = self
                    .cache
                    .get(&self.edit_path)
                    .map(|v| {
                        v.iter()
                            .filter(|it| {
                                (self.show_hidden || !it.is_hidden)
                                    && (!self.dirs_only || it.is_dir)
                            })
                            .count()
                    })
                    .unwrap_or(0);
                ui.separator();
                ui.label(format!("Items: {}", count));
            });
        });
    }
}
