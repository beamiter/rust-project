mod egui_bar;
use egui::{FontFamily, FontId, TextStyle};
use egui::{Margin, Pos2};
pub use egui_bar::MyEguiApp;
use std::collections::BTreeMap;
use std::env;
use FontFamily::Monospace;

use font_kit::source::SystemSource;

// 获取屏幕宽度的函数
fn get_screen_width() -> f32 {
    #[cfg(target_os = "windows")]
    {
        use winapi::um::winuser::GetSystemMetrics;
        use winapi::um::winuser::SM_CXSCREEN;
        unsafe { GetSystemMetrics(SM_CXSCREEN) as f32 }
    }

    #[cfg(target_os = "linux")]
    {
        use x11rb::connection::Connection;
        let (conn, screen_num) = x11rb::connect(None).unwrap();
        let screen = &conn.setup().roots[screen_num];
        println!("{}", screen_num);
        // 获取屏幕的宽度和高度，以像素为单位
        let width_in_pixels = screen.width_in_pixels as f32;
        let height_in_pixels = screen.height_in_pixels as f32;

        // 获取屏幕的物理宽度和高度，以毫米为单位
        let width_in_mm = screen.width_in_millimeters as f32;
        let height_in_mm = screen.height_in_millimeters as f32;

        // 计算水平和垂直方向的DPI
        let dpi_x = (width_in_pixels / (width_in_mm / 25.4)) as f32;
        let dpi_y = (height_in_pixels / (height_in_mm / 25.4)) as f32;

        // 对于大多数情况，可以采用水平和垂直方向DPI的平均值
        let dpi = (dpi_x + dpi_y) / 2.0;

        // 一般情况下，标准DPI是96，因此缩放因子可以通过如下方式计算：
        let scale_factor = dpi / 96.0;

        println!("Width in pixels: {}", width_in_pixels);
        println!("Height in pixels: {}", height_in_pixels);
        println!("Width in mm: {}", width_in_mm);
        println!("Height in mm: {}", height_in_mm);
        println!("DPI (X direction): {}", dpi_x);
        println!("DPI (Y direction): {}", dpi_y);
        println!("Average DPI: {}", dpi);
        println!("Scale factor: {}", scale_factor);
        screen.width_in_pixels as f32
    }

    #[cfg(target_os = "macos")]
    {
        use core_graphics::display::{CGDisplay, CGMainDisplayID};
        let display_id = CGMainDisplayID();
        let display = CGDisplay::new(display_id);
        display.pixels_wide() as f32
    }
}

fn load_system_nerd_font(ctx: &egui::Context) -> Result<(), Box<dyn std::error::Error>> {
    let mut fonts = egui::FontDefinitions::default();

    // 查找系统字体
    let system_source = SystemSource::new();

    // 寻找Nerd Font
    let font_handle = system_source.select_best_match(
        &[font_kit::family_name::FamilyName::Title(
            "SauceCodePro Nerd Font".to_string(),
        )],
        &font_kit::properties::Properties::new(),
    )?;

    // 获取字体数据
    let font = font_handle.load()?;
    let font_data = font.copy_font_data().ok_or("Failed to copy font data")?;

    // 添加到egui
    fonts.font_data.insert(
        "nerd-font".to_owned(),
        egui::FontData::from_owned(font_data.to_vec()).into(),
    );

    // 配置字体族
    // fonts
    //     .families
    //     .get_mut(&egui::FontFamily::Proportional)
    //     .unwrap()
    //     .insert(0, "nerd-font".to_owned());
    fonts
        .families
        .get_mut(&egui::FontFamily::Monospace)
        .unwrap()
        .insert(0, "nerd-font".to_owned());

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

const SCALE_FACTOR: f32 = 1.66666666;

fn main() -> eframe::Result {
    let args: Vec<String> = env::args().collect();
    let pipe_path = args.get(1).cloned().unwrap_or_else(|| "".to_string());
    let screen_width = get_screen_width() / SCALE_FACTOR;
    println!("screen_width: {}", screen_width);
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            // .with_window_type(egui::X11WindowType::Dock)
            .with_position(Pos2::new(0., 0.))
            .with_inner_size([screen_width, MyEguiApp::FONT_SIZE + 18.]) // Initial height
            .with_min_inner_size([screen_width, MyEguiApp::FONT_SIZE + 18.]) // Minimum size
            // .with_max_inner_size([f32::INFINITY, 20.0]) // Set max height to 20.0
            .with_decorations(false), // Hide title bar and decorations
        // .with_always_on_top(), // Keep window always on top
        ..Default::default()
    };
    eframe::run_native(
        "My egui App",
        native_options,
        Box::new(|cc| {
            let _ = load_system_nerd_font(&cc.egui_ctx);
            configure_text_styles(&cc.egui_ctx);
            Ok(Box::new(MyEguiApp::new(pipe_path.to_string())))
        }),
    )
}
