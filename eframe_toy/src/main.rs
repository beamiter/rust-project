#![warn(clippy::all, rust_2018_idioms)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

#[derive(serde::Deserialize, serde::Serialize, Clone, Copy, PartialEq, Eq)]
enum ActiveApp {
    SSHCommander,
    Filer,
    ImageViewer,
    EditorInstaller,
}

// 持久化 AppShell 的轻量状态（仅保存当前激活的子应用）
#[derive(serde::Deserialize, serde::Serialize)]
struct AppShellState {
    active: ActiveApp,
}

impl Default for AppShellState {
    fn default() -> Self {
        Self {
            active: ActiveApp::Filer, // 默认打开哪个子应用可按需调整
        }
    }
}

const APP_SHELL_KEY: &str = "APP_SHELL_STATE_V1";

struct AppShell {
    active: ActiveApp,
    ssh: toy::SSHCommander,
    filer: toy::Filer,
    viewer: toy::ImageViewerApp,
    editor_installer: toy::EditorsInstallerApp,
}

impl AppShell {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // 全局字体样式统一配置（之前在 main 中为 Filer/ImageViewer 配置）
        toy::configure_text_styles(&cc.egui_ctx);

        // SSHCommander 支持从 storage 恢复，沿用其 new(cc) 逻辑
        let ssh = toy::SSHCommander::new(cc);

        // 其他子应用用默认
        let filer = toy::Filer::default();
        let viewer = toy::ImageViewerApp::default();
        let editor_installer = toy::EditorsInstallerApp::default();

        // 从 storage 恢复上次选择的子应用
        let active = if let Some(storage) = cc.storage {
            eframe::get_value::<AppShellState>(storage, APP_SHELL_KEY)
                .unwrap_or_default()
                .active
        } else {
            ActiveApp::Filer
        };

        Self {
            active,
            ssh,
            filer,
            viewer,
            editor_installer,
        }
    }
}

impl eframe::App for AppShell {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        // 1) 持久化当前激活的子应用
        let shell_state = AppShellState {
            active: self.active,
        };
        eframe::set_value(storage, APP_SHELL_KEY, &shell_state);

        // 2) 将 SSHCommander 的状态保存到原先使用的 APP_KEY
        // 这样 SSHCommander::new(cc) 仍能从相同 key 恢复其内部状态
        eframe::set_value(storage, eframe::APP_KEY, &self.ssh);
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // 顶部“应用选择”条
        egui::TopBottomPanel::top("app_selector_top").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("Eframe Toy", |ui| {
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.add_space(16.0);

                egui::ComboBox::from_label("Select Toy")
                    .selected_text(match self.active {
                        ActiveApp::SSHCommander => "SSH Commander",
                        ActiveApp::Filer => "Filer",
                        ActiveApp::ImageViewer => "Image Viewer",
                        ActiveApp::EditorInstaller => "Editor Installer",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.active,
                            ActiveApp::SSHCommander,
                            "SSH Commander",
                        );
                        ui.selectable_value(&mut self.active, ActiveApp::Filer, "Filer");
                        ui.selectable_value(
                            &mut self.active,
                            ActiveApp::ImageViewer,
                            "Image Viewer",
                        );
                        ui.selectable_value(
                            &mut self.active,
                            ActiveApp::EditorInstaller,
                            "Editor Installer",
                        );
                    });

                ui.separator();
                ui.add_space(16.0);

                egui::widgets::global_theme_preference_buttons(ui);
            });
        });

        // 转发给当前激活的子应用
        match self.active {
            ActiveApp::SSHCommander => self.ssh.update(ctx, frame),
            ActiveApp::Filer => self.filer.update(ctx, frame),
            ActiveApp::ImageViewer => self.viewer.update(ctx, frame),
            ActiveApp::EditorInstaller => self.editor_installer.update(ctx, frame),
        }
    }
}

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_min_inner_size([400.0, 300.0]),
        ..Default::default()
    };

    eframe::run_native(
        "eframe_toy",
        native_options,
        Box::new(|cc| Ok(Box::new(AppShell::new(cc)))),
    )
}
