use crate::clash::core::ClashCore;
use crate::ui::{
    dashboard::Dashboard,
    proxies::Proxies,
    // logs::Logs, rules::Rules, settings::Settings,
};
use eframe::{CreationContext, egui};
use font_kit::source::SystemSource;
use log::info;
use std::sync::{Arc, Mutex};

/// 应用程序的主要视图
enum View {
    Dashboard,
    Proxies,
    Rules,
    Logs,
    Settings,
}

pub struct ClashApp {
    view: View,
    core: Arc<Mutex<ClashCore>>,
    dashboard: Arc<Mutex<Dashboard>>,
    proxies: Arc<Mutex<Proxies>>,
    // rules: Rules,
    // logs: Logs,
    // settings: Settings,
    is_running: bool,
}

impl ClashApp {
    pub fn new(cc: &CreationContext) -> Self {
        // 设置自定义字体和样式
        Self::configure_fonts_and_style(&cc.egui_ctx);

        // 初始化 Clash Core
        let core = Arc::new(Mutex::new(ClashCore::new()));

        // 初始化各个视图组件
        let dashboard = Arc::new(Mutex::new(Dashboard::new(Arc::clone(&core))));
        let proxies = Arc::new(Mutex::new(Proxies::new(Arc::clone(&core))));

        // 设置API客户端的应用状态
        if let Ok(core_guard) = core.lock() {
            if let Ok(mut api_client) = core_guard.get_api_client().lock() {
                api_client.set_app_state(Arc::clone(&proxies));
            }
        }
        // let rules = Rules::new(Arc::clone(&core));
        // let logs = Logs::new(Arc::clone(&core));
        // let settings = Settings::new(Arc::clone(&core));

        Self {
            view: View::Dashboard,
            core,
            dashboard,
            proxies,
            // rules,
            // logs,
            // settings,
            is_running: false,
        }
    }

    fn configure_fonts_and_style(ctx: &egui::Context) {
        // 加载自定义字体
        let mut fonts = egui::FontDefinitions::default();
        // 可以在这里添加自定义字体
        let system_source = SystemSource::new();
        for font_name in [
            "Noto Sans CJK SC".to_string(),
            "Noto Sans CJK TC".to_string(),
            "SauceCodeProNerdFont".to_string(),
            "DejaVuSansMonoNerdFont".to_string(),
            "JetBrainsMonoNerdFont".to_string(),
        ] {
            let font_handle = system_source.select_best_match(
                &[font_kit::family_name::FamilyName::Title(font_name.clone())],
                &font_kit::properties::Properties::new(),
            );
            if font_handle.is_err() {
                continue;
            }
            let font = font_handle.unwrap().load();
            if font.is_err() {
                continue;
            }
            let font_data = font.unwrap().copy_font_data();
            if font_data.is_none() {
                continue;
            }
            fonts.font_data.insert(
                font_name.clone(),
                egui::FontData::from_owned(font_data.unwrap().to_vec()).into(),
            );
            fonts
                .families
                .get_mut(&egui::FontFamily::Proportional)
                .unwrap()
                .insert(0, font_name.clone());
            fonts
                .families
                .get_mut(&egui::FontFamily::Monospace)
                .unwrap()
                .insert(0, font_name);
        }

        // 应用字体
        ctx.set_fonts(fonts);

        // 设置样式
        let mut style = (*ctx.style()).clone();
        style.text_styles = [
            (
                egui::TextStyle::Heading,
                egui::FontId::new(18.0, egui::FontFamily::Proportional),
            ),
            (
                egui::TextStyle::Body,
                egui::FontId::new(14.0, egui::FontFamily::Proportional),
            ),
            (
                egui::TextStyle::Monospace,
                egui::FontId::new(12.0, egui::FontFamily::Monospace),
            ),
            (
                egui::TextStyle::Button,
                egui::FontId::new(14.0, egui::FontFamily::Proportional),
            ),
            (
                egui::TextStyle::Small,
                egui::FontId::new(10.0, egui::FontFamily::Proportional),
            ),
        ]
        .into();
        ctx.set_style(style);
    }

    fn render_sidebar(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            // info!("here 0");
            ui.add_space(10.0);
            ui.heading("Clash");
            ui.add_space(20.0);

            // 状态指示器和开关
            ui.horizontal(|ui| {
                let status_text = if self.is_running {
                    "is running"
                } else {
                    "not running"
                };
                let status_color = if self.is_running {
                    egui::Color32::GREEN
                } else {
                    egui::Color32::RED
                };

                ui.label(egui::RichText::new("Status: ").strong());
                ui.colored_label(status_color, status_text);

                if ui
                    .add(egui::Button::new(if self.is_running {
                        "stop"
                    } else {
                        "start"
                    }))
                    .clicked()
                {
                    // info!("here 1");
                    self.toggle_clash();
                    // info!("here 2");
                }
            });

            ui.add_space(20.0);
            ui.separator();
            ui.add_space(10.0);

            // 导航菜单
            // info!("here 3");
            self.sidebar_button(ui, "dashboard", View::Dashboard);
            self.sidebar_button(ui, "proxies", View::Proxies);
            self.sidebar_button(ui, "rules", View::Rules);
            self.sidebar_button(ui, "logs", View::Logs);
            self.sidebar_button(ui, "settings", View::Settings);
            // info!("here 4");

            // 底部版本信息
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                ui.add_space(10.0);
                ui.label("Clash GUI v0.1.0");
                ui.add_space(5.0);
            });
        });
    }

    fn sidebar_button(&mut self, ui: &mut egui::Ui, text: &str, view: View) {
        let is_selected =
            matches!(&self.view, v if std::mem::discriminant(v) == std::mem::discriminant(&view));

        let mut button = egui::Button::new(text);
        if is_selected {
            button = button.fill(ui.style().visuals.selection.bg_fill);
        }

        if ui.add_sized([150.0, 30.0], button).clicked() {
            self.view = view;
        }
    }

    fn toggle_clash(&mut self) {
        info!("toggle_clash start");
        if self.is_running {
            if let Ok(mut core) = self.core.lock() {
                let _ = core.stop();
            }
        } else {
            if let Ok(mut core) = self.core.lock() {
                let _ = core.start();
            }
        }
        self.is_running = !self.is_running;
        info!("toggle_clash end");
    }
}

impl eframe::App for ClashApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 左侧边栏
        egui::SidePanel::left("sidebar")
            .resizable(false)
            .default_width(200.0)
            .show(ctx, |ui| {
                self.render_sidebar(ui);
            });

        // 主内容区域
        egui::CentralPanel::default().show(ctx, |ui| match self.view {
            View::Dashboard => self.dashboard.lock().unwrap().ui(ui, ctx),
            View::Proxies => self.proxies.lock().unwrap().ui(ui, ctx),
            _ => {} // View::Rules => self.rules.ui(ui, ctx),
                    // View::Logs => self.logs.ui(ui, ctx),
                    // View::Settings => self.settings.ui(ui, ctx),
        });
    }
}
