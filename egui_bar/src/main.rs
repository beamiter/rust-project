mod egui_bar;
mod audio_manager;
use chrono::Local;
use egui::{FontFamily, FontId, TextStyle};
use egui::{Margin, Pos2};
use egui_bar::constants::FONT_SIZE;
pub use egui_bar::MyEguiApp;
use flexi_logger::{Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming};
use font_kit::source::SystemSource;
use log::debug;
use log::error;
use log::info;
use log::warn;
use shared_structures::{SharedMessage, SharedRingBuffer};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{env, thread, u128};
use FontFamily::Monospace;
use FontFamily::Proportional;

fn load_system_nerd_font(ctx: &egui::Context) -> Result<(), Box<dyn std::error::Error>> {
    let mut fonts = egui::FontDefinitions::default();
    let system_source = SystemSource::new();
    // println!("all fonts: {:?}", system_source.all_fonts());
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
    info!("{:?}", fonts.families);
    ctx.set_fonts(fonts);
    Ok(())
}

fn configure_text_styles(ctx: &egui::Context) {
    ctx.all_styles_mut(move |style| {
        let text_styles: BTreeMap<TextStyle, FontId> = [
            (TextStyle::Body, FontId::new(FONT_SIZE, Monospace)),
            (TextStyle::Monospace, FontId::new(FONT_SIZE, Monospace)),
            (TextStyle::Button, FontId::new(FONT_SIZE, Monospace)),
            (TextStyle::Small, FontId::new(FONT_SIZE / 2., Proportional)),
            (
                TextStyle::Heading,
                FontId::new(FONT_SIZE * 2., Proportional),
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
    let now = Local::now();
    let timestamp = now.format("%Y-%m-%d_%H_%M_%S").to_string();
    let file_name = Path::new(&shared_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let log_filename = format!("egui_bar_{file_name}_{timestamp}");
    info!("{log_filename}");
    Logger::try_with_str("info")
        .unwrap()
        .format(flexi_logger::colored_opt_format)
        .log_to_file(
            FileSpec::default()
                .directory("/tmp")
                .basename(format!("{log_filename}"))
                .suffix("log"),
        )
        .duplicate_to_stdout(Duplicate::Info)
        // .log_to_stdout()
        // .buffer_capacity(1024)
        // .use_background_worker(true)
        .rotate(
            Criterion::Size(10_000_000),
            Naming::Numbers,
            Cleanup::KeepLogFiles(5),
        )
        .start()
        .unwrap();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_position(Pos2::new(0., 0.))
            .with_inner_size([800., FONT_SIZE * 2.]) // Initial height
            .with_min_inner_size([480., FONT_SIZE]) // Minimum size
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

            let (sender_msg, receiver_msg) = mpsc::channel();
            let (sender_resize, receiver_resize) = mpsc::channel();
            // 创建通道用于心跳检测
            let (tx, rx) = mpsc::channel();
            let egui_ctx = cc.egui_ctx.clone();

            thread::spawn(move || {
                // 创建或打开无锁环形缓冲区
                let ring_buffer: Option<SharedRingBuffer> = {
                    if shared_path.is_empty() {
                        None
                    } else {
                        match SharedRingBuffer::open(&shared_path) {
                            Ok(rb) => Some(rb),
                            Err(e) => {
                                error!("无法打开共享环形缓冲区: {}", e);
                                None
                            }
                        }
                    }
                };

                // 设置 panic 钩子
                let default_hook = std::panic::take_hook();
                std::panic::set_hook(Box::new(move |panic_info| {
                    default_hook(panic_info);
                    // 不需要发送任何消息，线程死亡会导致通道关闭
                }));

                let mut prev_timestamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis();
                let mut last_secs = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                let mut frame: u128 = 0;

                // 用于记录错误日志的计数器，避免日志过多
                let mut error_count = 0;
                let max_error_logs = 5;

                loop {
                    let cur_secs = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs();

                    let mut need_request_repaint = false;

                    if let Some(rb) = ring_buffer.as_ref() {
                        // 尝试从环形缓冲区读取数据
                        match rb.try_read_latest_message::<SharedMessage>() {
                            Ok(Some(message)) => {
                                // 检查时间戳是否更新
                                if prev_timestamp != message.timestamp {
                                    prev_timestamp = message.timestamp;
                                    info!("send message: {:?}", message);
                                    if sender_msg.send(message).is_ok() {
                                        need_request_repaint = true;
                                        error_count = 0; // 重置错误计数
                                    } else {
                                        error!("Fail to send message");
                                    }
                                }
                            }
                            Ok(None) => {
                                // 没有新数据，正常情况
                                if frame.wrapping_rem(1000) == 0 {
                                    debug!("No new message in ring buffer");
                                }
                            }
                            Err(e) => {
                                // 限制错误日志数量
                                if error_count < max_error_logs {
                                    error!("读取环形缓冲区错误: {}", e);
                                    error_count += 1;
                                } else if error_count == max_error_logs {
                                    error!("读取环形缓冲区持续出错，后续错误将不再记录");
                                    error_count += 1;
                                }
                            }
                        }
                    } else if frame.wrapping_rem(100) == 0 {
                        error!("环形缓冲区未初始化");
                    }

                    if frame.wrapping_rem(100) == 0 {
                        info!("frame {frame}: {last_secs}, {cur_secs}");
                    }

                    if cur_secs != last_secs {
                        need_request_repaint = true;
                    }

                    while let Ok(_) = receiver_resize.try_recv() {}

                    if need_request_repaint {
                        warn!("request_repaint");
                        egui_ctx.request_repaint_after(Duration::from_micros(1));
                    }

                    last_secs = cur_secs;
                    frame = frame.wrapping_add(1).wrapping_rem(u128::MAX);

                    if tx.send(()).is_err() {
                        // 如果发送失败，说明接收端已关闭
                        break;
                    }

                    thread::sleep(Duration::from_millis(10));
                }
            });

            thread::spawn(move || {
                // 主线程监控心跳
                loop {
                    match rx.recv_timeout(Duration::from_secs(2)) {
                        Ok(_) => {
                            // 收到心跳，继续运行
                        }
                        Err(_) => {
                            // 超时或通道关闭，表示线程可能已死亡
                            error!("sub thread died, killing main thread now");
                            std::process::exit(1);
                        }
                    }
                }
            });

            Ok(Box::new(MyEguiApp::new(cc, receiver_msg, sender_resize)))
        }),
    )
}
