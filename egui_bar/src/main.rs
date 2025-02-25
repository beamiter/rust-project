mod egui_bar;
use bincode::deserialize;
use egui::{FontFamily, FontId, TextStyle};
use egui::{Margin, Pos2};
pub use egui_bar::MyEguiApp;
use shared_memory::{Shmem, ShmemConf};
use shared_structures::SharedMessage;
use std::collections::BTreeMap;
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{env, thread};
use FontFamily::Monospace;
use FontFamily::Proportional;

use font_kit::source::SystemSource;

fn load_system_nerd_font(ctx: &egui::Context) -> Result<(), Box<dyn std::error::Error>> {
    let mut fonts = egui::FontDefinitions::default();
    let system_source = SystemSource::new();
    // println!("all fonts: {:?}", system_source.all_fonts());
    for font_name in [
        "Noto Sans CJK SC".to_string(),
        "Noto Sans CJK TC".to_string(),
        "SauceCodeProNerdFont".to_string(),
        "JetBrainsMonoNerdFont".to_string(),
    ] {
        let font_handle = system_source.select_best_match(
            &[font_kit::family_name::FamilyName::Title(font_name.clone())],
            &font_kit::properties::Properties::new(),
        )?;
        let font = font_handle.load()?;
        let font_data = font.copy_font_data().ok_or("Failed to copy font data")?;
        fonts.font_data.insert(
            font_name.clone(),
            egui::FontData::from_owned(font_data.to_vec()).into(),
        );
        // fonts
        //     .families
        //     .get_mut(&egui::FontFamily::Proportional)
        //     .unwrap()
        //     .insert(0, "nerd-font".to_owned());
        fonts
            .families
            .get_mut(&egui::FontFamily::Monospace)
            .unwrap()
            .insert(0, font_name);
    }
    println!("{:?}", fonts.families);
    ctx.set_fonts(fonts);
    Ok(())
}

fn configure_text_styles(ctx: &egui::Context) {
    ctx.all_styles_mut(move |style| {
        let text_styles: BTreeMap<TextStyle, FontId> = [
            (
                TextStyle::Body,
                FontId::new(MyEguiApp::FONT_SIZE, Monospace),
            ),
            (
                TextStyle::Monospace,
                FontId::new(MyEguiApp::FONT_SIZE, Monospace),
            ),
            (
                TextStyle::Button,
                FontId::new(MyEguiApp::FONT_SIZE, Monospace),
            ),
            (
                TextStyle::Small,
                FontId::new(MyEguiApp::FONT_SIZE / 2., Proportional),
            ),
            (
                TextStyle::Heading,
                FontId::new(MyEguiApp::FONT_SIZE * 2., Proportional),
            ),
        ]
        .into();
        style.text_styles = text_styles;
        // style.spacing.item_spacing = egui::vec2(8.0, 0.0);
        // style.spacing.window_margin = Margin::symmetric(0., 0.);
        // style.spacing.window_margin = Margin::ZERO;
        style.spacing.window_margin = Margin::same(0.0);
        style.spacing.menu_spacing = 0.0;
        style.spacing.menu_margin = Margin::same(0.0);
    });
}

fn main() -> eframe::Result {
    let args: Vec<String> = env::args().collect();
    let shared_path = args.get(1).cloned().unwrap_or_else(|| "".to_string());
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_transparent(true)
            .with_window_type(egui::X11WindowType::Dock)
            .with_position(Pos2::new(0., 0.))
            .with_inner_size([800., MyEguiApp::FONT_SIZE]) // Initial height
            .with_min_inner_size([800., MyEguiApp::FONT_SIZE]) // Minimum size
            // .with_max_inner_size([f32::INFINITY, 20.0]) // Set max height to 20.0
            .with_decorations(false), // Hide title bar and decorations
        // .with_always_on_top(), // Keep window always on top
        vsync: true,
        ..Default::default()
    };

    eframe::run_native(
        "egui_bar",
        native_options,
        Box::new(|cc| {
            let _ = load_system_nerd_font(&cc.egui_ctx);
            configure_text_styles(&cc.egui_ctx);

            let (sender, receiver) = mpsc::channel();
            let egui_ctx = cc.egui_ctx.clone();
            thread::spawn(move || {
                let shmem: Option<Shmem> = {
                    if shared_path.is_empty() {
                        None
                    } else {
                        Some(ShmemConf::new().flink(shared_path.clone()).open().unwrap())
                    }
                };

                let mut prev_timestamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis();
                let mut last_secs = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                loop {
                    let cur_secs = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs();

                    let mut need_request_repaint = false;
                    if let Some(shmem) = shmem.as_ref() {
                        let data = shmem.as_ptr();
                        let serialized = unsafe { std::slice::from_raw_parts(data, shmem.len()) };
                        let message: SharedMessage = deserialize(serialized).unwrap();
                        if prev_timestamp != message.timestamp {
                            prev_timestamp = message.timestamp;
                            // println!("send message: {:?}", message);
                            let _ = sender.send(message);
                            need_request_repaint = true;
                        }
                    }

                    // println!("{}, {}", last_secs, cur_secs);
                    if cur_secs != last_secs {
                        need_request_repaint = true;
                    }
                    if need_request_repaint {
                        // println!("request_repaint");
                        egui_ctx.request_repaint_after(Duration::from_micros(1));
                    }
                    last_secs = cur_secs;
                    thread::sleep(Duration::from_millis(10));
                }
            });

            Ok(Box::new(MyEguiApp::new(cc, receiver)))
        }),
    )
}
