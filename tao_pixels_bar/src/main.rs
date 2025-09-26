use anyhow::Result;
use cairo::{Context, Format, ImageSurface};
use log::warn;
use pango::FontDescription;
use pixels::wgpu::TextureFormat;
use pixels::{Pixels, PixelsBuilder, SurfaceTexture};
use shared_structures::SharedRingBuffer;
use std::env;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tao::{
    dpi::{LogicalSize, PhysicalSize},
    event::{Event, StartCause, WindowEvent},
    event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopProxy},
    window::{WindowBuilder, WindowId},
};
use xbar_core::{
    AppState, BarConfig, ShapeStyle, default_colors, draw_bar, initialize_logging,
    spawn_shared_eventfd_notifier,
};

#[derive(Debug, Clone, Copy)]
enum UserEvent {
    SharedUpdated,
    Tick,
}

// 共享 eventfd 线程：读取 eventfd 通知并发出 UserEvent::SharedUpdated
fn spawn_shared_thread(proxy: EventLoopProxy<UserEvent>, shared_efd: Option<i32>) {
    if let Some(efd) = shared_efd {
        thread::spawn(move || {
            let mut buf8 = [0u8; 8];
            let mut pfd = libc::pollfd {
                fd: efd,
                events: libc::POLLIN,
                revents: 0,
            };
            loop {
                let pr = unsafe { libc::poll(&mut pfd as *mut libc::pollfd, 1, -1) };
                if pr < 0 {
                    let err = std::io::Error::last_os_error();
                    if let Some(code) = err.raw_os_error() {
                        if code == libc::EINTR {
                            continue;
                        }
                    }
                    warn!("[shared-thread] poll error: {}", err);
                    thread::sleep(Duration::from_millis(50));
                    continue;
                }
                if (pfd.revents & libc::POLLIN) != 0 {
                    let r = unsafe { libc::read(efd, buf8.as_mut_ptr() as *mut _, buf8.len()) };
                    if r == 8 {
                        let _ = proxy.send_event(UserEvent::SharedUpdated);
                    } else if r < 0 {
                        let err = std::io::Error::last_os_error();
                        if let Some(code) = err.raw_os_error() {
                            if code == libc::EINTR {
                                continue;
                            }
                        }
                        warn!("[shared-thread] eventfd read error: {}", err);
                        thread::sleep(Duration::from_millis(50));
                    }
                }
            }
        });
    }
}

// 每秒对齐 tick 线程：按秒对齐，发送 UserEvent::Tick
fn spawn_tick_thread(proxy: EventLoopProxy<UserEvent>) {
    thread::spawn(move || {
        let mut last_bucket: u64 = 0;
        loop {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_else(|_| Duration::from_secs(0));
            let bucket = now.as_secs(); // 或按分钟
            if bucket != last_bucket {
                last_bucket = bucket;
                let _ = proxy.send_event(UserEvent::Tick);
            }
            // sleep 到下一秒边界
            let subns = now.subsec_nanos() as u64;
            let remain_ns = 1_000_000_000u64.saturating_sub(subns).max(1);
            thread::sleep(Duration::from_nanos(remain_ns));
        }
    });
}

struct App {
    // 仅保存窗口 ID（Window 由 pixels 的 SurfaceTexture 持有）
    window_id: Option<WindowId>,

    // 配置与状态
    colors: xbar_core::Colors,
    cfg: BarConfig,
    font: FontDescription,
    state: AppState,

    // DPI/尺寸
    scale_factor: f64,
    logical_size: LogicalSize<f64>,
    last_physical_size: PhysicalSize<u32>,

    // 系统监控更新时间
    last_monitor_update: Instant,

    // 最近一次鼠标物理坐标
    last_cursor_pos_px: Option<(i32, i32)>,

    // pixels 渲染
    pixels: Option<Pixels<'static>>,
    pixels_w: u32,
    pixels_h: u32,

    // 时间刷新 bucket（按秒或分钟），减少无谓重绘
    last_time_bucket: u64,
}

impl App {
    fn new(
        shared_buffer: Option<Arc<SharedRingBuffer>>,
        logical_size: LogicalSize<f64>,
        scale: f64,
    ) -> Self {
        let colors = default_colors();
        let cfg = BarConfig {
            bar_height: 40,
            padding_x: 8.0,
            padding_y: 4.0,
            tag_spacing: 6.0,
            pill_hpadding: 10.0,
            pill_radius: 8.0,
            shape_style: ShapeStyle::Pill,
            time_icon: "",
            screenshot_label: " Screenshot",
        };
        let font = FontDescription::from_string("JetBrainsMono Nerd Font 11");
        let state = AppState::new(shared_buffer);

        Self {
            window_id: None,
            colors,
            cfg,
            font,
            state,
            scale_factor: scale,
            logical_size,
            last_physical_size: PhysicalSize::new(
                logical_size.width.round() as u32,
                logical_size.height.round() as u32,
            ),
            last_monitor_update: Instant::now(),
            last_cursor_pos_px: None,
            pixels: None,
            pixels_w: 0,
            pixels_h: 0,
            last_time_bucket: 0,
        }
    }

    // 初始化窗口 + pixels（将 Window 所有权交给 SurfaceTexture/Pixels）
    fn ensure_init_window(&mut self, target: &tao::event_loop::EventLoopWindowTarget<UserEvent>) {
        if self.window_id.is_some() {
            return;
        }

        // 使用主显示器宽度和 bar 高度
        let primary = target
            .primary_monitor()
            .or_else(|| target.available_monitors().next());
        let scale = primary.as_ref().map(|m| m.scale_factor()).unwrap_or(1.0);
        self.scale_factor = scale;

        let screen_size: PhysicalSize<u32> = primary
            .as_ref()
            .map(|m| m.size())
            .unwrap_or(PhysicalSize::new(1920, 1080));
        let width_px = screen_size.width;
        let height_px = self.cfg.bar_height as u32;

        self.logical_size = LogicalSize::new(
            (width_px as f64) / self.scale_factor,
            (height_px as f64) / self.scale_factor,
        );
        self.last_physical_size = PhysicalSize::new(width_px, height_px);

        let window = WindowBuilder::new()
            .with_title("tao_pixels_bar")
            .with_inner_size(self.logical_size)
            .with_decorations(false)
            .with_resizable(true)
            .with_visible(true)
            .with_transparent(false)
            .build(target)
            .expect("create window failed");
        let win_id = window.id();

        // 构建 pixels（移动 Window 所有权）
        let width_px = (self.logical_size.width * self.scale_factor).round() as u32;
        let height_px = (self.logical_size.height * self.scale_factor).round() as u32;
        let surface_texture = SurfaceTexture::new(width_px, height_px, window);
        let pixels: Pixels<'static> = PixelsBuilder::new(width_px, height_px, surface_texture)
            .texture_format(TextureFormat::Bgra8UnormSrgb)
            .enable_vsync(true)
            .request_adapter_options(pixels::wgpu::RequestAdapterOptions {
                power_preference: pixels::wgpu::PowerPreference::LowPower,
                ..Default::default()
            })
            .build()
            .map_err(|e| anyhow::anyhow!("pixels::new: {}", e))
            .expect("pixels create failed");

        self.window_id = Some(win_id);
        self.pixels_w = width_px;
        self.pixels_h = height_px;
        self.pixels = Some(pixels);

        // 初始化时间 bucket 并首次绘制
        self.last_time_bucket = self.current_time_bucket();
        if let Err(e) = self.redraw() {
            warn!("redraw error (initial): {}", e);
        }
    }

    // Cairo 绘制到 pixels 帧缓冲
    fn redraw(&mut self) -> anyhow::Result<()> {
        if self.window_id.is_none() {
            return Ok(());
        }

        let width_px = (self.logical_size.width * self.scale_factor).round() as i32;
        let height_px = (self.logical_size.height * self.scale_factor).round() as i32;
        let stride = width_px
            .checked_mul(4)
            .ok_or_else(|| anyhow::anyhow!("stride overflow"))?;

        if let Some(pixels) = self.pixels.as_mut() {
            // 将对 frame 的可变借用限制在该作用域内，避免和 pixels.render() 冲突
            {
                let frame: &mut [u8] = pixels.frame_mut();

                // 用 frame 的裸指针创建临时 Cairo ImageSurface
                let surface = unsafe {
                    ImageSurface::create_for_data_unsafe(
                        frame.as_mut_ptr(),
                        Format::ARgb32, // BGRA (pre-multiplied) on little-endian
                        width_px,
                        height_px,
                        stride,
                    )?
                };

                // Cairo 绘制
                let cr = Context::new(&surface)?;
                cr.save()?;
                cr.set_source_rgba(0.0, 0.0, 0.0, 1.0);
                cr.set_operator(cairo::Operator::Source);
                cr.paint()?;
                cr.restore()?;

                let w_u16 = (width_px as u32).min(u16::MAX as u32) as u16;
                let h_u16 = (height_px as u32).min(u16::MAX as u32) as u16;
                draw_bar(
                    &cr,
                    w_u16,
                    h_u16,
                    &self.colors,
                    &mut self.state,
                    &self.font,
                    &self.cfg,
                )?;

                surface.flush();
            }

            pixels
                .render()
                .map_err(|e| anyhow::anyhow!("pixels render: {}", e))?;
        }

        Ok(())
    }

    // 悬停更新 + 重绘
    fn update_hover_and_redraw(&mut self, px: i32, py: i32) {
        if self.state.update_hover(px as i16, py as i16) {
            if let Err(e) = self.redraw() {
                warn!("redraw error (hover): {}", e);
            }
        }
    }

    // 点击处理 + 可能触发重绘
    fn handle_button(&mut self, px: i32, py: i32, button_id: u8) {
        // 记录 show_seconds 切换前的值
        let prev_show_seconds = self.state.show_seconds;

        if self.state.handle_buttons(px as i16, py as i16, button_id) {
            // 若 show_seconds 改变，立即更新时间 bucket，避免下次 tick 再二次重绘
            if self.state.show_seconds != prev_show_seconds {
                self.last_time_bucket = self.current_time_bucket();
            }
            if let Err(e) = self.redraw() {
                warn!("redraw error (button): {}", e);
            }
        }
    }

    // 根据 show_seconds 计算时间 bucket（秒或分钟）
    fn current_time_bucket(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| Duration::from_secs(0));
        if self.state.show_seconds {
            now.as_secs()
        } else {
            now.as_secs() / 60
        }
    }
}

fn main() -> Result<()> {
    // 参数
    let args: Vec<String> = env::args().collect();
    let shared_path = args.get(1).cloned().unwrap_or_default();

    // 日志
    if let Err(e) = initialize_logging("tao_pixels_bar", &shared_path) {
        eprintln!("Failed to initialize logging: {}", e);
        std::process::exit(1);
    }

    // 共享内存与通知
    let shared_buffer = SharedRingBuffer::create_shared_ring_buffer_aux(&shared_path).map(Arc::new);
    let shared_efd = spawn_shared_eventfd_notifier(shared_buffer.clone(), false);

    // 事件循环与代理（tao）
    let event_loop: EventLoop<UserEvent> = EventLoopBuilder::with_user_event().build();
    let proxy = event_loop.create_proxy();

    // 后台线程：SharedUpdated + Tick
    spawn_shared_thread(proxy.clone(), shared_efd);
    spawn_tick_thread(proxy.clone());

    // 初始逻辑尺寸（实际在 Init/Resumed 中按主显示器设置）
    let logical_size = LogicalSize::new(800.0, 40.0);
    let mut app = App::new(shared_buffer, logical_size, 1.0);

    event_loop.run(move |event, target, control_flow| {
        // 始终等待事件（共享通知 + Tick + 窗口事件）
        *control_flow = ControlFlow::Wait;

        match event {
            Event::NewEvents(StartCause::Init) => {
                app.ensure_init_window(target);
            }
            Event::Resumed => {
                app.ensure_init_window(target);
            }

            Event::UserEvent(ue) => match ue {
                UserEvent::SharedUpdated => {
                    let mut need_redraw = false;
                    if let Some(buf_arc) = app.state.shared_buffer.as_ref().cloned() {
                        match buf_arc.try_read_latest_message() {
                            Ok(Some(msg)) => {
                                log::trace!("redraw by msg: {:?}", msg);
                                app.state.update_from_shared(msg);
                                need_redraw = true;
                            }
                            Ok(None) => { /* 没有消息 */ }
                            Err(e) => {
                                warn!("Shared try_read_latest_message failed: {}", e);
                            }
                        }
                    }
                    if need_redraw {
                        if let Err(e) = app.redraw() {
                            warn!("redraw error (SharedUpdated): {}", e);
                        }
                    }
                }
                UserEvent::Tick => {
                    let mut need_redraw = false;

                    // 时间 bucket 变化才重绘（秒或分钟由 state.show_seconds 决定）
                    let bucket = app.current_time_bucket();
                    if bucket != app.last_time_bucket {
                        app.last_time_bucket = bucket;
                        log::trace!("redraw by time bucket update: {}", bucket);
                        need_redraw = true;
                    }

                    // 系统监控定期更新（2s）
                    if app.last_monitor_update.elapsed() >= Duration::from_secs(2) {
                        app.state.system_monitor.update_if_needed();
                        app.state.audio_manager.update_if_needed();
                        app.last_monitor_update = Instant::now();
                        log::trace!("maybe redraw by system update");
                        need_redraw = true;
                    }

                    if need_redraw {
                        if let Err(e) = app.redraw() {
                            warn!("redraw error (Tick): {}", e);
                        }
                    }
                }
            },

            Event::WindowEvent {
                window_id, event, ..
            } => {
                if Some(window_id) != app.window_id {
                    return;
                }

                match event {
                    WindowEvent::CloseRequested => {
                        *control_flow = ControlFlow::Exit;
                    }
                    WindowEvent::Resized(new_size) => {
                        app.last_physical_size = new_size;
                        app.logical_size = new_size.to_logical::<f64>(app.scale_factor);

                        if let Some(pixels) = app.pixels.as_mut() {
                            let w = (app.logical_size.width * app.scale_factor).round() as u32;
                            let h = (app.logical_size.height * app.scale_factor).round() as u32;
                            if app.pixels_w != w || app.pixels_h != h {
                                if let Err(e) = pixels.resize_surface(w, h) {
                                    warn!("pixels.resize_surface error: {}", e);
                                }
                                if let Err(e) = pixels.resize_buffer(w, h) {
                                    warn!("pixels.resize_buffer error: {}", e);
                                }
                                app.pixels_w = w;
                                app.pixels_h = h;
                            }
                        }

                        if let Err(e) = app.redraw() {
                            warn!("redraw error (Resized): {}", e);
                        }
                    }
                    WindowEvent::ScaleFactorChanged {
                        scale_factor,
                        new_inner_size,
                    } => {
                        let new_physical = *new_inner_size;
                        app.scale_factor = scale_factor;
                        app.last_physical_size = new_physical;
                        app.logical_size = new_physical.to_logical::<f64>(app.scale_factor);

                        if let Some(pixels) = app.pixels.as_mut() {
                            let w = (app.logical_size.width * app.scale_factor).round() as u32;
                            let h = (app.logical_size.height * app.scale_factor).round() as u32;
                            if app.pixels_w != w || app.pixels_h != h {
                                if let Err(e) = pixels.resize_surface(w, h) {
                                    warn!("pixels.resize_surface error: {}", e);
                                }
                                if let Err(e) = pixels.resize_buffer(w, h) {
                                    warn!("pixels.resize_buffer error: {}", e);
                                }
                                app.pixels_w = w;
                                app.pixels_h = h;
                            }
                        }

                        if let Err(e) = app.redraw() {
                            warn!("redraw error (ScaleFactorChanged): {}", e);
                        }
                    }
                    WindowEvent::CursorMoved { position, .. } => {
                        let px = position.x.round() as i32;
                        let py = position.y.round() as i32;
                        app.last_cursor_pos_px = Some((px, py));
                        app.update_hover_and_redraw(px, py);
                        log::trace!("cursor px={}, py={}", px, py);
                    }
                    WindowEvent::CursorLeft { .. } => {
                        app.state.clear_hover();
                        if let Err(e) = app.redraw() {
                            warn!("redraw error (CursorLeft): {}", e);
                        }
                    }
                    WindowEvent::MouseInput { state, button, .. } => {
                        use tao::event::{ElementState, MouseButton};
                        if state == ElementState::Pressed {
                            if let Some((px, py)) = app.last_cursor_pos_px {
                                let button_id = match button {
                                    MouseButton::Left => 1,
                                    MouseButton::Middle => 2,
                                    MouseButton::Right => 3,
                                    MouseButton::Other(n) => n as u8,
                                    _ => todo!(),
                                };
                                app.handle_button(px, py, button_id);
                            }
                        }
                    }
                    _ => {}
                }
            }

            Event::RedrawRequested(_) => {
                if let Err(e) = app.redraw() {
                    warn!("redraw error (RedrawRequested): {}", e);
                }
            }

            Event::LoopDestroyed => {
                // 资源在 Drop 时释放
            }

            _ => {}
        }
    });
}
