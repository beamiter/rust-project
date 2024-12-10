#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release
#![allow(rustdoc::missing_crate_level_docs)] // it's an example
#![allow(unsafe_code)]
#![allow(clippy::undocumented_unsafe_blocks)]

use eframe::egui;

use ssh2::Session;
use std::{io::Read, net::TcpStream};

fn main() -> eframe::Result {
    env_logger::init(); // Log to stderr (if you run with `RUST_LOG=debug`).
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([350.0, 380.0]),
        multisampling: 4,
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "Customized Toy",
        options,
        Box::new(|cc| Ok(Box::new(MyApp::new(cc)))),
    )
}

struct MyApp {
    host: String,
    username: String,
    password: String,
    command: String,
    output: String,
}

impl MyApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            host: "10.21.31.17:22".into(),
            username: "root".into(),
            password: "1".into(),
            command: "docker exec fpp-container-mnt-data-maf_planning ./sim fpp play -v --ddp-time 2 --product MNP backup/bags/1210/PLEAW5372_event_light_recording_20241206-144429_0.bag".into(),
            output: String::new(),
        }
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

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
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
        });
    }
}
