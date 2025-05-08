#![warn(clippy::all, rust_2018_idioms)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value_t = 1)]
    instance: u8,
}

// When compiling natively:
#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_min_inner_size([400.0, 300.0]),
        ..Default::default()
    };

    let args = Args::parse();
    match args.instance {
        0 => eframe::run_native(
            "ssh commander",
            native_options,
            Box::new(|cc| Ok(Box::new(toy::SSHCommander::new(cc)))),
        ),
        1 => eframe::run_native(
            "image processor",
            native_options,
            Box::new(|cc| Ok(Box::new(toy::ImageProcessor::new(cc)))),
        ),
        2 => eframe::run_native(
            "filer",
            native_options,
            Box::new(|cc| {
                toy::configure_text_styles(&cc.egui_ctx);
                Ok(Box::<toy::Filer>::default())
            }),
        ),
        3 => eframe::run_native(
            "image viewer",
            native_options,
            Box::new(|cc| {
                toy::configure_text_styles(&cc.egui_ctx);
                Ok(Box::<toy::ImageViewerApp>::default())
            }),
        ),
        _ => {
            panic!("unsupported instance");
        }
    }
}

// When compiling to web using trunk:
#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast as _;

    // Redirect `log` message to `console.log` and friends:
    eframe::WebLogger::init(log::LevelFilter::Debug).ok();

    let web_options = eframe::WebOptions::default();

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .expect("No window")
            .document()
            .expect("No document");

        let canvas = document
            .get_element_by_id("the_canvas_id")
            .expect("Failed to find the_canvas_id")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("the_canvas_id was not a HtmlCanvasElement");

        let start_result = eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(|cc| Ok(Box::new(toy::SSHCommander::new(cc)))),
            )
            .await;

        // Remove the loading text and spinner:
        if let Some(loading_text) = document.get_element_by_id("loading_text") {
            match start_result {
                Ok(_) => {
                    loading_text.remove();
                }
                Err(e) => {
                    loading_text.set_inner_html(
                        "<p> The app has crashed. See the developer console for details. </p>",
                    );
                    panic!("Failed to start eframe: {e:?}");
                }
            }
        }
    });
}
