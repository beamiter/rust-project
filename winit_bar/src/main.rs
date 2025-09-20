use anyhow::Result;
use cairo::{Context, Format, ImageSurface};
use log::warn;
use pango::FontDescription;
use shared_structures::{SharedMessage, SharedRingBuffer};
use std::env;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::wrapper::ConnectionExt as _;

use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalSize},
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    window::{Window, WindowAttributes, WindowId},
};

use x11rb::{connection::Connection, protocol::xproto::PropMode, rust_connection::RustConnection};

use xbar_core::{
    AppState, BarConfig, ShapeStyle, default_colors, draw_bar, initialize_logging,
    spawn_shared_eventfd_notifier,
};

#[derive(Debug, Clone, Copy)]
enum UserEvent {
    Tick,
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

// X11: 设置 dock/strut（EWMH），仅在 X11 下有效
fn set_x11_dock_properties(
    conn: &RustConnection,
    window_id: u32,
    width_px: u32,
    height_px: u32,
) -> Result<()> {
    let intern = |name: &str| -> Result<u32> {
        let reply = conn.intern_atom(false, name.as_bytes())?.reply()?;
        Ok(reply.atom)
    };

    let net_wm_window_type = intern("_NET_WM_WINDOW_TYPE")?;
    let net_wm_window_type_dock = intern("_NET_WM_WINDOW_TYPE_DOCK")?;
    let net_wm_state = intern("_NET_WM_STATE")?;
    let net_wm_state_above = intern("_NET_WM_STATE_ABOVE")?;
    let net_wm_desktop = intern("_NET_WM_DESKTOP")?;
    let net_wm_strut_partial = intern("_NET_WM_STRUT_PARTIAL")?;
    let net_wm_strut = intern("_NET_WM_STRUT")?;
    let net_wm_name = intern("_NET_WM_NAME")?;
    let utf8_string = intern("UTF8_STRING")?;
    let atom = intern("ATOM")?;
    let cardinal = intern("CARDINAL")?;

    conn.change_property32(
        PropMode::REPLACE,
        window_id,
        net_wm_window_type,
        atom,
        &[net_wm_window_type_dock],
    )?;
    conn.change_property32(
        PropMode::REPLACE,
        window_id,
        net_wm_state,
        atom,
        &[net_wm_state_above],
    )?;
    conn.change_property32(
        PropMode::REPLACE,
        window_id,
        net_wm_desktop,
        cardinal,
        &[0xFFFFFFFF],
    )?;

    let top = height_px;
    let top_start_x = 0u32;
    let top_end_x = width_px.saturating_sub(1);
    let strut_partial = [
        0,
        0,
        top,
        0, // left, right, top, bottom
        0,
        0,
        0,
        0, // left_start_y, left_end_y, right_start_y, right_end_y
        top_start_x,
        top_end_x, // top_start_x, top_end_x
        0,
        0, // bottom_start_x, bottom_end_x
    ];
    conn.change_property32(
        PropMode::REPLACE,
        window_id,
        net_wm_strut_partial,
        cardinal,
        &strut_partial,
    )?;
    let strut = [0u32, 0, top, 0];
    conn.change_property32(PropMode::REPLACE, window_id, net_wm_strut, cardinal, &strut)?;
    conn.change_property8(
        PropMode::REPLACE,
        window_id,
        net_wm_name,
        utf8_string,
        b"winit_bar",
    )?;
    conn.flush()?;
    Ok(())
}

fn spawn_tick_thread(proxy: EventLoopProxy<UserEvent>) {
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_millis(250));
            let _ = proxy.send_event(UserEvent::Tick);
        }
    });
}

fn spawn_shared_thread(proxy: EventLoopProxy<UserEvent>, shared_efd: Option<i32>) {
    if let Some(efd) = shared_efd {
        thread::spawn(move || {
            let mut buf8 = [0u8; 8];
            loop {
                let r =
                    unsafe { libc::read(efd, buf8.as_mut_ptr() as *mut libc::c_void, buf8.len()) };
                if r == 8 {
                    let _ = proxy.send_event(UserEvent::SharedUpdated);
                    continue;
                } else if r < 0 {
                    let err = std::io::Error::last_os_error();
                    if let Some(code) = err.raw_os_error() {
                        if code == libc::EINTR {
                            continue;
                        }
                        if code == libc::EAGAIN {
                            // 非阻塞且当前无事件：正常，避免 warn 刷屏
                            thread::sleep(Duration::from_millis(5));
                            continue;
                        }
                    }
                    warn!("[shared-thread] eventfd read error: {}", err);
                    thread::sleep(Duration::from_millis(50));
                } else {
                    // 未读满 8 字节，罕见，稍作等待
                    thread::sleep(Duration::from_millis(5));
                }
            }
        });
    }
}

struct App {
    // 运行期资源
    window: Option<Window>,
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

    // 更新计时
    last_clock_update: Instant,
    last_monitor_update: Instant,

    // 记录最近一次鼠标物理坐标（像素）
    last_cursor_pos_px: Option<(i32, i32)>,
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
            last_clock_update: Instant::now(),
            last_monitor_update: Instant::now(),
            last_cursor_pos_px: None,
        }
    }

    fn redraw(&mut self) -> Result<()> {
        let window = match &self.window {
            Some(w) => w,
            None => return Ok(()),
        };

        let width_px = (self.logical_size.width * self.scale_factor).round() as u32;
        let height_px = (self.logical_size.height * self.scale_factor).round() as u32;

        if self.back.is_none() {
            self.back = Some(CairoBackBuffer::new(width_px, height_px)?);
        } else {
            self.back
                .as_mut()
                .unwrap()
                .ensure_size(width_px, height_px)?;
        }
        let back = self.back.as_mut().unwrap();

        // 1) 临时创建 Context
        {
            let cr = Context::new(&back.image)?;

            // 清屏（注意这里用了 Source + 不透明黑）
            cr.save()?;
            cr.set_source_rgba(0.0, 0.0, 0.0, 1.0);
            cr.set_operator(cairo::Operator::Source);
            cr.paint()?;
            cr.restore()?;

            // 绘制 bar
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
            // 离开作用域，cr 被 drop，引用计数回到 1
        }

        // 2) 现在可以安全读取像素数据
        back.image.flush();
        let stride = back.image.stride() as usize;
        let data = back.image.data()?; // 不再 NonExclusive

        // 3) softbuffer 显示
        let context = softbuffer::Context::new(window)
            .map_err(|e| anyhow::anyhow!("softbuffer::Context::new: {}", e))?;
        let mut surface = softbuffer::Surface::new(&context, window)
            .map_err(|e| anyhow::anyhow!("softbuffer::Surface::new: {}", e))?;

        let (Some(w_nz), Some(h_nz)) = (
            std::num::NonZeroU32::new(width_px),
            std::num::NonZeroU32::new(height_px),
        ) else {
            return Ok(());
        };
        surface
            .resize(w_nz, h_nz)
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        {
            let mut buf = surface.buffer_mut().map_err(|e| anyhow::anyhow!("{}", e))?;
            let w = width_px as usize;
            let h = height_px as usize;
            for y in 0..h {
                let src_row = &data[y * stride..y * stride + w * 4];
                let dst_row = &mut buf[y * w..(y + 1) * w];

                for x in 0..w {
                    let i = x * 4;
                    dst_row[x] = u32::from_le_bytes([
                        src_row[i + 0], // B
                        src_row[i + 1], // G
                        src_row[i + 2], // R
                        src_row[i + 3], // A
                    ]);
                }
            }
            buf.present().map_err(|e| anyhow::anyhow!("{}", e))?;
        }

        Ok(())
    }

    fn update_hover_and_redraw(&mut self, px: i32, py: i32) {
        let hovered = self.state.ss_rect.contains(px as i16, py as i16);
        if hovered != self.state.is_ss_hover {
            self.state.is_ss_hover = hovered;
            if let Err(e) = self.redraw() {
                warn!("redraw error (hover): {}", e);
            }
        }
    }

    fn handle_button(&mut self, px: i32, py: i32, button_id: u8) {
        if self.state.handle_buttons(px as i16, py as i16, button_id) {
            if let Err(e) = self.redraw() {
                warn!("redraw error (button): {}", e);
            }
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // 若尚未创建窗口，创建并初始化
        if self.window.is_none() {
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
                width_px as f64 / self.scale_factor,
                height_px as f64 / self.scale_factor,
            );

            let attrs = WindowAttributes::default()
                .with_title("winit_bar")
                .with_inner_size(self.logical_size)
                .with_decorations(false)
                .with_resizable(true)
                .with_visible(true)
                .with_transparent(false);

            let window = event_loop
                .create_window(attrs)
                .expect("create_window failed");

            // X11: 设置 dock/strut（Wayland 下无效）
            #[cfg(target_os = "linux")]
            {
                #[allow(unused_imports)]
                use winit::platform::x11::WindowExtX11;
                let x11_win_id = u64::from(window.id());
                if let Ok((conn, _screen)) = x11rb::connect(None) {
                    let _ = set_x11_dock_properties(&conn, x11_win_id as u32, width_px, height_px);
                }
            }

            // Cairo back buffer
            let back = CairoBackBuffer::new(width_px, height_px).expect("cairo back buffer failed");

            self.window_id = Some(window.id());
            self.window = Some(window);
            self.back = Some(back);

            // 首次绘制
            if let Err(e) = self.redraw() {
                warn!("redraw error (initial): {}", e);
            }
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Tick => {
                let mut need_redraw = false;
                if self.last_clock_update.elapsed() >= Duration::from_secs(1) {
                    self.last_clock_update = Instant::now();
                    log::info!("redraw by clock update: {:?}", self.last_clock_update);
                    need_redraw = true;
                }
                if self.last_monitor_update.elapsed() >= Duration::from_secs(2) {
                    self.state.system_monitor.update_if_needed();
                    self.state.audio_manager.update_if_needed();
                    self.last_monitor_update = Instant::now();
                    log::info!("redraw by system update: {:?}", self.last_monitor_update);
                    need_redraw = true;
                }
                if need_redraw {
                    if let Err(e) = self.redraw() {
                        warn!("redraw error (Tick): {}", e);
                    }
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
                        log::info!("redraw by msg: {:?}", msg);
                        self.state.update_from_shared(msg);
                        need_redraw = true;
                    }
                }
                if need_redraw {
                    if let Err(e) = self.redraw() {
                        warn!("redraw error (SharedUpdated): {}", e);
                    }
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
        let window = match &self.window {
            Some(w) => w,
            None => return,
        };

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::Resized(new_size) => {
                // 物理尺寸 -> 逻辑尺寸
                let scale = window.scale_factor();
                self.scale_factor = scale;
                self.logical_size = new_size.to_logical::<f64>(scale);
                if let Err(e) = self.redraw() {
                    warn!("redraw error (Resized): {}", e);
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.scale_factor = scale_factor;
                let logical = window.inner_size().to_logical::<f64>(self.scale_factor);
                self.logical_size = logical;
                if let Err(e) = self.redraw() {
                    warn!("redraw error (ScaleFactorChanged): {}", e);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale = window.scale_factor();
                let px = (position.x * scale) as i32;
                let py = (position.y * scale) as i32;
                // 记录位置供 MouseInput 使用
                self.last_cursor_pos_px = Some((px, py));
                self.update_hover_and_redraw(px, py);
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
                // 某些平台会发该事件；我们已在需要时 redraw，这里可忽略或补绘
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
    if let Err(e) = initialize_logging("winit_bar", &shared_path) {
        eprintln!("Failed to initialize logging: {}", e);
        std::process::exit(1);
    }

    // 共享内存与通知
    let shared_buffer = SharedRingBuffer::create_shared_ring_buffer_aux(&shared_path).map(Arc::new);
    let shared_efd = spawn_shared_eventfd_notifier(shared_buffer.clone());

    // 事件循环与代理（winit 0.30.12）
    let event_loop: EventLoop<UserEvent> = EventLoop::with_user_event().build()?;
    let proxy = event_loop.create_proxy();

    // 后台线程：Tick 与 SharedUpdated
    spawn_tick_thread(proxy.clone());
    spawn_shared_thread(proxy.clone(), shared_efd);

    // 初始逻辑尺寸，先用一个占位，实际在 resumed 中根据显示器设置
    let logical_size = LogicalSize::new(800.0, 40.0);
    let mut app = App::new(shared_buffer, logical_size, 1.0);

    // 运行
    event_loop.run_app(&mut app)?;
    Ok(())
}
