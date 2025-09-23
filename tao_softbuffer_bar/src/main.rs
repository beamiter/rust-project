use anyhow::Result;
use cairo::{Context, Format, ImageSurface};
use log::warn;
use pango::FontDescription;
use shared_structures::{SharedMessage, SharedRingBuffer};
use std::env;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tao::event_loop::EventLoopBuilder;

use tao::{
    dpi::{LogicalSize, PhysicalSize},
    event::{Event, StartCause, WindowEvent},
    event_loop::{ControlFlow, EventLoop, EventLoopProxy},
    window::{Window, WindowBuilder, WindowId},
};

use xbar_core::{
    AppState, BarConfig, ShapeStyle, default_colors, draw_bar, initialize_logging,
    spawn_shared_eventfd_notifier,
};

type SbSurface = softbuffer::Surface<Rc<Window>, Rc<Window>>;

#[derive(Debug, Clone, Copy)]
enum UserEvent {
    SharedUpdated,
}

// Cairo 后备缓冲：ImageSurface + Context
struct CairoBackBuffer {
    width: u32,
    height: u32,
    image: ImageSurface,
}
impl CairoBackBuffer {
    fn new(width: u32, height: u32) -> Result<Self> {
        let image = ImageSurface::create(Format::ARgb32, width as i32, height as i32)?;
        Ok(Self {
            width,
            height,
            image,
        })
    }
    fn ensure_size(&mut self, width: u32, height: u32) -> Result<()> {
        if self.width == width && self.height == height {
            return Ok(());
        }
        self.image = ImageSurface::create(Format::ARgb32, width as i32, height as i32)?;
        self.width = width;
        self.height = height;
        Ok(())
    }
    #[allow(dead_code)]
    fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
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
    // 运行期资源
    window: Option<Rc<Window>>,
    window_id: Option<WindowId>,
    back: Option<CairoBackBuffer>,

    // 配置与状态
    colors: xbar_core::Colors,
    cfg: BarConfig,
    font: FontDescription,
    state: AppState,

    // DPI/尺寸
    scale_factor: f64,
    logical_size: LogicalSize<f64>,

    // 更新时间控制
    last_monitor_update: Instant,
    last_time_bucket: u64, // 按秒或分钟的时间 bucket（由 show_seconds 决定）

    // 记录最近一次鼠标物理坐标（像素）
    last_cursor_pos_px: Option<(i32, i32)>,

    soft_surface: Option<SbSurface>,
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
            window: None,
            window_id: None,
            back: None,
            colors,
            cfg,
            font,
            state,
            scale_factor: scale,
            logical_size,
            last_monitor_update: Instant::now(),
            last_time_bucket: 0,
            last_cursor_pos_px: None,
            soft_surface: None,
        }
    }

    fn ensure_init_window(&mut self, target: &tao::event_loop::EventLoopWindowTarget<UserEvent>) {
        if self.window.is_some() {
            return;
        }

        // 初始尺寸：以主显示器宽度 + bar 高度
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
            width_px as f64 / self.scale_factor,
            height_px as f64 / self.scale_factor,
        );

        let window = WindowBuilder::new()
            .with_title("tao_softbuffer_bar")
            .with_inner_size(self.logical_size)
            .with_decorations(false)
            .with_resizable(true)
            .with_visible(true)
            .with_transparent(false)
            .build(target)
            .expect("create window failed");
        let window = Rc::new(window);

        // softbuffer Context 与 Surface
        let soft_ctx = softbuffer::Context::new(window.clone())
            .map_err(|e| anyhow::anyhow!("softbuffer::Context::new: {}", e))
            .expect("softbuffer context");
        let mut soft_surface = softbuffer::Surface::new(&soft_ctx, window.clone())
            .map_err(|e| anyhow::anyhow!("softbuffer::Surface::new: {}", e))
            .expect("softbuffer surface");

        let width_px = (self.logical_size.width * self.scale_factor).round() as u32;
        let height_px = (self.logical_size.height * self.scale_factor).round() as u32;
        if let (Some(w), Some(h)) = (NonZeroU32::new(width_px), NonZeroU32::new(height_px)) {
            let _ = soft_surface.resize(w, h);
        }

        // Cairo back buffer
        let back = CairoBackBuffer::new(width_px, height_px).expect("cairo back buffer failed");

        self.window_id = Some(window.id());
        self.window = Some(window);
        self.back = Some(back);
        self.soft_surface = Some(soft_surface);

        // 初始化时间 bucket 并首次绘制
        self.last_time_bucket = self.current_time_bucket();
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn redraw(&mut self) -> Result<()> {
        if self.window.is_none() {
            return Ok(());
        }
        let width_px = (self.logical_size.width * self.scale_factor).round() as u32;
        let height_px = (self.logical_size.height * self.scale_factor).round() as u32;

        // back buffer 尺寸保证
        if self.back.is_none() {
            self.back = Some(CairoBackBuffer::new(width_px, height_px)?);
        } else {
            self.back
                .as_mut()
                .unwrap()
                .ensure_size(width_px, height_px)?;
        }
        let back = self.back.as_mut().unwrap();

        // Cairo 绘制
        {
            let cr = Context::new(&back.image)?;
            cr.save()?;
            cr.set_source_rgba(0.0, 0.0, 0.0, 1.0);
            cr.set_operator(cairo::Operator::Source);
            cr.paint()?;
            cr.restore()?;

            let w_u16 = width_px.min(u16::MAX as u32) as u16;
            let h_u16 = height_px.min(u16::MAX as u32) as u16;
            draw_bar(
                &cr,
                w_u16,
                h_u16,
                &self.colors,
                &mut self.state,
                &self.font,
                &self.cfg,
            )?;
        }

        // 像素提交
        back.image.flush();
        let stride = back.image.stride() as usize;
        let data = back.image.data()?; // &[u8]
        let w = width_px as usize;
        let h = height_px as usize;

        let surface = match self.soft_surface.as_mut() {
            Some(s) => s,
            None => return Ok(()),
        };

        use bytemuck::cast_slice;
        let mut buf = surface.buffer_mut().map_err(|e| anyhow::anyhow!("{}", e))?;
        if stride == w * 4 {
            let src_u32: &[u32] = cast_slice(&data[..h * stride]); // BGRA 小端
            buf[..w * h].copy_from_slice(src_u32);
        } else {
            for y in 0..h {
                let row = &data[y * stride..y * stride + w * 4];
                let src_u32: &[u32] = cast_slice(row);
                let dst_row = &mut buf[y * w..(y + 1) * w];
                dst_row.copy_from_slice(src_u32);
            }
        }
        buf.present().map_err(|e| anyhow::anyhow!("{}", e))?;
        Ok(())
    }

    fn update_hover_and_redraw(&mut self, px: i32, py: i32) {
        if self.state.update_hover(px as i16, py as i16) {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }

    fn handle_button(&mut self, px: i32, py: i32, button_id: u8) {
        // 记录 show_seconds 切换前的值
        let prev_show_seconds = self.state.show_seconds;
        if self.state.handle_buttons(px as i16, py as i16, button_id) {
            // 若 show_seconds 改变，立即更新时间 bucket，避免等待到下一次唤醒才更新
            if self.state.show_seconds != prev_show_seconds {
                self.last_time_bucket = self.current_time_bucket();
            }
            if let Some(w) = &self.window {
                w.request_redraw();
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

// 计算下一次时间 bucket 的边界 Instant（对齐秒或分钟）
fn next_time_bucket_instant(show_seconds: bool) -> Instant {
    let now_inst = Instant::now();
    let now_sys = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0));
    let subns = now_sys.subsec_nanos() as u64;

    if show_seconds {
        // 到下一秒边界
        let remain_ns = 1_000_000_000u64.saturating_sub(subns).max(1);
        now_inst + Duration::from_nanos(remain_ns)
    } else {
        // 到下一分钟边界
        let sec_in_min = (now_sys.as_secs() % 60) as u64;
        let nanos_into_min = sec_in_min * 1_000_000_000u64 + subns;
        let remain_ns = 60_000_000_000u64.saturating_sub(nanos_into_min).max(1);
        now_inst + Duration::from_nanos(remain_ns)
    }
}

// 取“时间 bucket 边界”和“监控更新 2s”中的较早者作为唤醒时间
fn next_deadline(app: &App) -> Instant {
    let bucket_due = next_time_bucket_instant(app.state.show_seconds);
    let monitor_due = app.last_monitor_update + Duration::from_secs(2);
    std::cmp::min(bucket_due, monitor_due)
}

// 设置下一次等待截止时间
fn schedule_next_wake(app: &App, control_flow: &mut ControlFlow) {
    *control_flow = ControlFlow::WaitUntil(next_deadline(app));
}

fn main() -> Result<()> {
    // 参数
    let args: Vec<String> = env::args().collect();
    let shared_path = args.get(1).cloned().unwrap_or_default();

    // 日志
    if let Err(e) = initialize_logging("tao_softbuffer_bar", &shared_path) {
        eprintln!("Failed to initialize logging: {}", e);
        std::process::exit(1);
    }

    // 共享内存与通知
    let shared_buffer = SharedRingBuffer::create_shared_ring_buffer_aux(&shared_path).map(Arc::new);
    let shared_efd = spawn_shared_eventfd_notifier(shared_buffer.clone(), false);

    // 事件循环与代理（tao）
    let event_loop: EventLoop<UserEvent> = EventLoopBuilder::with_user_event().build();
    let proxy = event_loop.create_proxy();

    // 后台线程：仅 SharedUpdated
    spawn_shared_thread(proxy.clone(), shared_efd);

    // 初始逻辑尺寸，实际初始化在 NewEvents::Init 中完成
    let logical_size = LogicalSize::new(800.0, 40.0);
    let mut app = App::new(shared_buffer, logical_size, 1.0);

    // 运行（闭包式）
    event_loop.run(move |event, target, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::NewEvents(StartCause::Init) => {
                // 首次进入事件循环时创建窗口与渲染资源
                app.ensure_init_window(target);
                schedule_next_wake(&app, control_flow);
            }

            Event::Resumed => {
                // 某些平台用 Resumed 作为初始化契机
                app.ensure_init_window(target);
                schedule_next_wake(&app, control_flow);
            }

            // 定时唤醒：取代原先的 Tick 线程
            Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {
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
                    if let Some(w) = &app.window {
                        w.request_redraw();
                    }
                }

                schedule_next_wake(&app, control_flow);
            }

            Event::UserEvent(ue) => match ue {
                UserEvent::SharedUpdated => {
                    let mut need_redraw = false;
                    if let Some(buf_arc) = app.state.shared_buffer.as_ref().cloned() {
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
                            log::info!("redraw by msg: {:?}", msg);
                            app.state.update_from_shared(msg);
                            need_redraw = true;
                        }
                    }
                    if need_redraw {
                        if let Some(w) = &app.window {
                            w.request_redraw();
                        }
                    }
                    schedule_next_wake(&app, control_flow);
                }
            },

            Event::WindowEvent {
                window_id, event, ..
            } => {
                if Some(window_id) != app.window_id {
                    return;
                }
                let window = match &app.window {
                    Some(w) => w,
                    None => return,
                };

                match event {
                    WindowEvent::CloseRequested => {
                        *control_flow = ControlFlow::Exit;
                    }
                    WindowEvent::Resized(new_size) => {
                        app.scale_factor = window.scale_factor();
                        app.logical_size = new_size.to_logical::<f64>(app.scale_factor);
                        if let Some(surface) = app.soft_surface.as_mut() {
                            let w = (app.logical_size.width * app.scale_factor).round() as u32;
                            let h = (app.logical_size.height * app.scale_factor).round() as u32;
                            if let (Some(wnz), Some(hnz)) = (NonZeroU32::new(w), NonZeroU32::new(h))
                            {
                                let _ = surface.resize(wnz, hnz);
                            }
                        }
                        if let Err(e) = app.redraw() {
                            warn!("redraw error (Resized): {}", e);
                        }
                        schedule_next_wake(&app, control_flow);
                    }
                    WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                        app.scale_factor = scale_factor;
                        app.logical_size = window.inner_size().to_logical::<f64>(app.scale_factor);
                        if let Some(surface) = app.soft_surface.as_mut() {
                            let w = (app.logical_size.width * app.scale_factor).round() as u32;
                            let h = (app.logical_size.height * app.scale_factor).round() as u32;
                            if let (Some(wnz), Some(hnz)) = (NonZeroU32::new(w), NonZeroU32::new(h))
                            {
                                let _ = surface.resize(wnz, hnz);
                            }
                        }
                        if let Err(e) = app.redraw() {
                            warn!("redraw error (ScaleFactorChanged): {}", e);
                        }
                        schedule_next_wake(&app, control_flow);
                    }
                    WindowEvent::CursorMoved { position, .. } => {
                        let px = position.x.round() as i32;
                        let py = position.y.round() as i32;
                        app.last_cursor_pos_px = Some((px, py));
                        app.update_hover_and_redraw(px, py);
                        log::trace!("cursor px={}, py={}", px, py);
                        schedule_next_wake(&app, control_flow);
                    }
                    WindowEvent::CursorLeft { .. } => {
                        app.state.clear_hover();
                        schedule_next_wake(&app, control_flow);
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
                                let prev_show_seconds = app.state.show_seconds;
                                app.handle_button(px, py, button_id);
                                if app.state.show_seconds != prev_show_seconds {
                                    // show_seconds 改变，按新粒度重新调度唤醒
                                    schedule_next_wake(&app, control_flow);
                                }
                            }
                        }
                        schedule_next_wake(&app, control_flow);
                    }
                    _ => {
                        schedule_next_wake(&app, control_flow);
                    }
                }
            }
            Event::RedrawRequested(_) => {
                log::trace!("[RedrawRequested]");
                if let Err(e) = app.redraw() {
                    warn!("redraw error (RedrawRequested): {}", e);
                }
                schedule_next_wake(&app, control_flow);
            }

            Event::LoopDestroyed => {
                // 资源在 Drop 时释放，这里无需处理
            }

            _ => {}
        }
    });
}
