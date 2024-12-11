/// We derive Deserialize/Serialize so we can persist app state on shutdown.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)] // if we add new fields, give them default values when deserializing old state
pub struct TemplateApp {
    host: String,
    username: String,
    password: String,
    command: String,
    #[serde(skip)] // This how you opt-out of serialization of a field
    output: String,
}

impl Default for TemplateApp {
    fn default() -> Self {
        Self {
            host: "10.21.31.17:22".into(),
            username: "root".into(),
            password: "1".into(),
            command: "docker exec fpp-container-mnt-data-maf_planning ./sim fpp play -v --ddp-time 2 --product MNP backup/bags/1210/PLEAW5372_event_light_recording_20241206-144429_0.bag".into(),
            output: String::new(),
        }
    }
}

use ssh2::Session;
use std::{io::Read, net::TcpStream};

impl TemplateApp {
    /// Called once before the first frame.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // This is also where you can customize the look and feel of egui using
        // `cc.egui_ctx.set_visuals` and `cc.egui_ctx.set_fonts`.

        // Load previous app state (if any).
        // Note that you must enable the `persistence` feature for this to work.
        if let Some(storage) = cc.storage {
            return eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();
        }

        Default::default()
    }
    fn execute_command(&self) -> Result<String, Box<dyn std::error::Error>> {
        let tcp = TcpStream::connect(&self.host)?;

        let mut sess = Session::new()?;
        sess.set_tcp_stream(tcp);

        sess.handshake()?;

        sess.userauth_password(&self.username, &self.password)?;

        if !sess.authenticated() {
            return Err("Authentication failed".into());
        }

        let mut channel = sess.channel_session()?;
        channel.exec(&self.command)?;

        let mut output = String::new();
        channel.read_to_string(&mut output)?;

        channel.wait_close()?;

        Ok(output)
    }
}

fn parse_hyperlink(line: &str) -> Option<String> {
    let url_regex = regex::Regex::new(r"https?://[^\s]+").unwrap();
    if let Some(url_match) = url_regex.find(line) {
        return Some(url_match.as_str().to_string());
    }
    None
}

impl eframe::App for TemplateApp {
    /// Called by the frame work to save state before shutdown.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    /// Called each time the UI needs repainting, which may be many times per second.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Put your widgets into a `SidePanel`, `TopBottomPanel`, `CentralPanel`, `Window` or `Area`.
        // For inspiration and more examples, go to https://emilk.github.io/egui

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            // The top panel is often a good place for a menu bar:

            egui::menu::bar(ui, |ui| {
                // NOTE: no File->Quit on web pages!
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
            ui.heading("SSH Command Executor");

            ui.horizontal(|ui| {
                ui.label("Host: ");
                ui.text_edit_singleline(&mut self.host);
            });
            ui.horizontal(|ui| {
                ui.label("Username: ");
                ui.text_edit_singleline(&mut self.username);
            });
            ui.horizontal(|ui| {
                ui.label("Password: ");
                ui.text_edit_singleline(&mut self.password);
            });
            ui.horizontal(|ui| {
                ui.label("Command: ");
                ui.text_edit_singleline(&mut self.command);
            });

            if ui.button("Execute").clicked() {
                match self.execute_command() {
                    Ok(output) => self.output = output,
                    Err(e) => self.output = format!("Error executing command: {}", e),
                }
            }

            ui.separator();
            ui.heading("Output:");

            for line in self.output.lines() {
                if let Some(link) = parse_hyperlink(line) {
                    ui.hyperlink_to(line, link);
                } else {
                    ui.label(line);
                }
            }

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
