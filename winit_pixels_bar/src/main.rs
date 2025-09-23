use anyhow::Result;
use cairo::{Context, Format, ImageSurface};
use log::warn;
use pango::FontDescription;
use shared_structures::{SharedMessage, SharedRingBuffer};
use std::env;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use winit::window::Window;
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalSize},
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    window::{WindowAttributes, WindowId},
};

use xbar_core::{
    AppState, BarConfig, ShapeStyle, default_colors, draw_bar, initialize_logging,
    spawn_shared_eventfd_notifier,
};

use pixels::wgpu::TextureFormat;
use pixels::{Pixels, PixelsBuilder, SurfaceTexture};

#[derive(Debug, Clone, Copy)]
enum UserEvent {
    Tick,
    SharedUpdated,
}

// Tick 线程：始终按秒对齐唤醒（低开销），用 state.show_seconds 决定是否重绘
fn spawn_tick_thread(proxy: EventLoopProxy<UserEvent>) {
    thread::spawn(move || {
        loop {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_else(|_| Duration::from_secs(0));
            let subns = now.subsec_nanos() as u64;
            let sleep_dur = Duration::from_nanos(1_000_000_000u64.saturating_sub(subns));
            thread::sleep(sleep_dur);
            let _ = proxy.send_event(UserEvent::Tick);
        }
    });
}

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

struct App {
    // 仅保存窗口 ID
    window_id: Option<WindowId>,
    window: Option<Arc<Window>>,

    // 配置与状态
    colors: xbar_core::Colors,
    cfg: BarConfig,
    font: FontDescription,
    state: AppState,

    // DPI/尺寸
    scale_factor: f64,
    logical_size: LogicalSize<f64>,
    last_physical_size: PhysicalSize<u32>,

    // 系统监控更新计时
    last_monitor_update: Instant,

    // 最近一次鼠标物理坐标（像素）
    last_cursor_pos_px: Option<(i32, i32)>,

    // pixels（持有 Window 的所有权）
    pixels: Option<Pixels<'static>>,
    // 当前 pixels buffer 尺寸（只在 Resized/ScaleFactorChanged 时更新）
    pixels_w: u32,
    pixels_h: u32,

    // 时间刷新：根据 state.show_seconds 决定 bucket（秒或分钟）
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
            window: None,
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
            {
                let frame: &mut [u8] = pixels.frame_mut();
                let surface = unsafe {
                    ImageSurface::create_for_data_unsafe(
                        frame.as_mut_ptr(),
                        Format::ARgb32, // BGRA pre-multiplied on little-endian
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

    #[inline]
    fn request_redraw(&self) {
        if let Some(win) = self.window.as_ref() {
            win.request_redraw();
        }
    }

    fn update_hover_and_redraw(&mut self, px: i32, py: i32) {
        if self.state.update_hover(px as i16, py as i16) {
            // 改为仅请求重绘
            self.request_redraw();
        }
    }

    fn handle_button(&mut self, px: i32, py: i32, button_id: u8) {
        // 记录 show_seconds 切换前的值
        let prev_show_seconds = self.state.show_seconds;

        if self.state.handle_buttons(px as i16, py as i16, button_id) {
            // 如果 show_seconds 在点击后发生变化，则把时间桶对齐，避免下一个 Tick 再多重绘一次
            if self.state.show_seconds != prev_show_seconds {
                self.last_time_bucket = self.current_time_bucket();
            }
            // 改为仅请求重绘
            self.request_redraw();
        }
    }

    // 按 state.show_seconds 决定当前时间 bucket（单位秒或分钟）
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

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // 若尚未创建窗口，创建并初始化
        if self.window_id.is_none() {
            // 初始尺寸：主显示器宽度，bar 高度
            let primary = event_loop
                .primary_monitor()
                .or_else(|| event_loop.available_monitors().next());
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

            let attrs = WindowAttributes::default()
                .with_title("winit_pixels_bar")
                .with_inner_size(self.logical_size)
                .with_decorations(false)
                .with_resizable(true)
                .with_visible(true)
                .with_transparent(false);

            // 创建 Window（owned）
            let window = event_loop
                .create_window(attrs)
                .expect("create_window failed");
            let win_id = window.id();
            let arc = Arc::new(window);

            // 创建 pixels（将 Window 所有权移动给 SurfaceTexture/Pixels）
            let width_px = (self.logical_size.width * self.scale_factor).round() as u32;
            let height_px = (self.logical_size.height * self.scale_factor).round() as u32;

            let surface_texture = SurfaceTexture::new(width_px, height_px, arc.clone());
            self.window = Some(arc);
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

            // 初始化时间 bucket
            self.last_time_bucket = self.current_time_bucket();

            // 首次绘制：仅请求重绘
            self.request_redraw();
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Tick => {
                let mut need_redraw = false;

                // 时间 bucket 变化才重绘（秒或分钟由 state.show_seconds 决定）
                let bucket = self.current_time_bucket();
                if bucket != self.last_time_bucket {
                    self.last_time_bucket = bucket;
                    log::trace!("redraw by time bucket update: {}", bucket);
                    need_redraw = true;
                }

                // 系统监控更新（保持 2s），降低日志等级
                if self.last_monitor_update.elapsed() >= Duration::from_secs(2) {
                    self.state.system_monitor.update_if_needed();
                    self.state.audio_manager.update_if_needed();
                    self.last_monitor_update = Instant::now();
                    log::trace!("maybe redraw by system update");
                    need_redraw = true;
                }

                if need_redraw {
                    // 改为仅请求重绘
                    self.request_redraw();
                }
            }
            UserEvent::SharedUpdated => {
                let mut need_redraw = false;
                if let Some(buf_arc) = self.state.shared_buffer.as_ref().cloned() {
                    let mut last_msg: Option<SharedMessage> = None;
                    loop {
                        match buf_arc.try_read_latest_message() {
                            Ok(Some(msg)) => {
                                last_msg = Some(msg);
                                continue;
                            }
                            Ok(None) => break,
                            Err(e) => {
                                warn!("Shared try_read_latest_message failed: {}", e);
                                break;
                            }
                        }
                    }
                    if let Some(msg) = last_msg {
                        log::trace!("redraw by msg: {:?}", msg);
                        self.state.update_from_shared(msg);
                        need_redraw = true;
                    }
                }
                if need_redraw {
                    // 改为仅请求重绘
                    self.request_redraw();
                }
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if Some(window_id) != self.window_id {
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::Resized(new_size) => {
                // new_size 是物理像素尺寸；更新记录
                self.last_physical_size = new_size;

                // 基于当前 scale_factor 更新逻辑尺寸
                self.logical_size = new_size.to_logical::<f64>(self.scale_factor);

                // 调整 pixels surface/buffer（仅在此处）
                if let Some(pixels) = self.pixels.as_mut() {
                    let w = (self.logical_size.width * self.scale_factor).round() as u32;
                    let h = (self.logical_size.height * self.scale_factor).round() as u32;
                    if self.pixels_w != w || self.pixels_h != h {
                        if let Err(e) = pixels.resize_surface(w, h) {
                            warn!("pixels.resize_surface error: {}", e);
                        }
                        if let Err(e) = pixels.resize_buffer(w, h) {
                            warn!("pixels.resize_buffer error: {}", e);
                        }
                        self.pixels_w = w;
                        self.pixels_h = h;
                    }
                }

                // 改为仅请求重绘
                self.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                // 更新缩放因子
                self.scale_factor = scale_factor;

                // 计算逻辑尺寸
                self.logical_size = self.last_physical_size.to_logical::<f64>(self.scale_factor);

                // 调整 pixels surface/buffer（仅在此处）
                if let Some(pixels) = self.pixels.as_mut() {
                    let w = (self.logical_size.width * self.scale_factor).round() as u32;
                    let h = (self.logical_size.height * self.scale_factor).round() as u32;
                    if self.pixels_w != w || self.pixels_h != h {
                        if let Err(e) = pixels.resize_surface(w, h) {
                            warn!("pixels.resize_surface error: {}", e);
                        }
                        if let Err(e) = pixels.resize_buffer(w, h) {
                            warn!("pixels.resize_buffer error: {}", e);
                        }
                        self.pixels_w = w;
                        self.pixels_h = h;
                    }
                }

                // 改为仅请求重绘
                self.request_redraw();
            }
            WindowEvent::CursorMoved { position, .. } => {
                // position 是 PhysicalPosition<f64>
                let px = position.x.round() as i32;
                let py = position.y.round() as i32;
                self.last_cursor_pos_px = Some((px, py));
                self.update_hover_and_redraw(px, py);
                log::trace!(
                    "cursor px={}, py={}, time_rect={:?}, ss_rect={:?}",
                    px,
                    py,
                    self.state.time_rect,
                    self.state.ss_rect
                );
            }
            WindowEvent::MouseInput { state, button, .. } => {
                use winit::event::{ElementState, MouseButton};
                if state == ElementState::Pressed {
                    if let Some((px, py)) = self.last_cursor_pos_px {
                        let button_id = match button {
                            MouseButton::Left => 1,
                            MouseButton::Middle => 2,
                            MouseButton::Right => 3,
                            MouseButton::Back => 8,
                            MouseButton::Forward => 9,
                            MouseButton::Other(n) => n as u8,
                        };
                        self.handle_button(px, py, button_id);
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.redraw() {
                    warn!("redraw error (RedrawRequested): {}", e);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        // 周期性刷新由 Tick/SharedUpdated 驱动，这里无需操作
    }
}

fn main() -> Result<()> {
    // 参数
    let args: Vec<String> = env::args().collect();
    let shared_path = args.get(1).cloned().unwrap_or_default();

    // 日志
    if let Err(e) = initialize_logging("winit_pixels_bar", &shared_path) {
        eprintln!("Failed to initialize logging: {}", e);
        std::process::exit(1);
    }

    // 共享内存与通知
    let shared_buffer = SharedRingBuffer::create_shared_ring_buffer_aux(&shared_path).map(Arc::new);
    let shared_efd = spawn_shared_eventfd_notifier(shared_buffer.clone(), false);

    // 事件循环与代理（winit 0.30.12）
    let event_loop: EventLoop<UserEvent> = EventLoop::with_user_event().build()?;
    let proxy = event_loop.create_proxy();

    // 后台线程：Tick 与 SharedUpdated
    spawn_tick_thread(proxy.clone());
    spawn_shared_thread(proxy.clone(), shared_efd);

    // 初始逻辑尺寸，实际在 resumed 中根据显示器设置
    let logical_size = LogicalSize::new(800.0, 40.0);
    let mut app = App::new(shared_buffer, logical_size, 1.0);

    // 运行
    event_loop.run_app(&mut app)?;
    Ok(())
}
