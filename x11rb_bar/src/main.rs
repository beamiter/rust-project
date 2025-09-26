use anyhow::Result;
use cairo::ffi::{xcb_connection_t, xcb_visualtype_t};
use cairo::{Context, XCBConnection as CairoXCBConnection, XCBDrawable, XCBSurface, XCBVisualType};
use log::{debug, warn};
use pango::FontDescription;
use shared_structures::{SharedMessage, SharedRingBuffer};
use std::env;
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, AsRawFd};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use xbar_core::{
    AppState, BarConfig, ShapeStyle, arm_second_timer, default_colors, draw_bar,
    initialize_logging, spawn_shared_eventfd_notifier,
};

use libc;

use x11rb::connection::Connection;
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ChangeWindowAttributesAux, CreateGCAux, CreateWindowAux, EventMask, Gcontext,
    Pixmap, PropMode, Screen, Window, WindowClass,
};
use x11rb::wrapper::ConnectionExt as _;
use x11rb::xcb_ffi::XCBConnection;

// ---------------- Cairo XCB 桥接（x11rb 版本） ----------------
struct CairoXcb {
    cxcb_conn: CairoXCBConnection,
    visual: XCBVisualType,
    _visual_owner: Box<xcb::x::Visualtype>,
}

fn find_visual_by_id_and_depth_by_ffi(
    screen: &Screen,
    target_visual_id: u32,
    target_depth: u8,
) -> Option<x11rb::protocol::xproto::Visualtype> {
    for depth in &screen.allowed_depths {
        if depth.depth == target_depth {
            for visual in &depth.visuals {
                if visual.visual_id == target_visual_id {
                    return Some(visual.clone());
                }
            }
        }
    }
    None
}

fn build_cairo_xcb_by_ffi(xcb_conn: &XCBConnection, screen: &Screen) -> Result<CairoXcb> {
    let root_visual_id = screen.root_visual;
    let root_depth = screen.root_depth;
    let visual = find_visual_by_id_and_depth_by_ffi(screen, root_visual_id, root_depth)
        .ok_or_else(|| anyhow::anyhow!("Could not find visualtype for root_visual"))?;
    let raw_visual = xcb::x::Visualtype::new(
        visual.visual_id,
        unsafe { std::mem::transmute(u32::from(visual.class)) },
        visual.bits_per_rgb_value,
        visual.colormap_entries,
        visual.red_mask,
        visual.green_mask,
        visual.blue_mask,
    );

    let visual_owner = Box::new(raw_visual);
    let ptr = (&*visual_owner) as *const xcb::x::Visualtype as *mut xcb_visualtype_t;
    let cxcb_vis = unsafe { XCBVisualType::from_raw_none(ptr) };
    let raw = xcb_conn.get_raw_xcb_connection();
    let cxcb_conn = unsafe { CairoXCBConnection::from_raw_none(raw as *mut xcb_connection_t) };

    Ok(CairoXcb {
        cxcb_conn,
        visual: cxcb_vis,
        _visual_owner: visual_owner,
    })
}

// ---------------- 后备缓冲（Pixmap + 可复用 Cairo Surface/Context） ----------------
struct BackBuffer {
    pm: Pixmap,
    width: u16,
    height: u16,
    depth: u8,
    surface: Option<XCBSurface>,
    cr: Option<Context>,
}
impl BackBuffer {
    fn new(conn: &XCBConnection, screen: &Screen, win: Window, w: u16, h: u16) -> Result<Self> {
        let pm = conn.generate_id()?;
        conn.create_pixmap(screen.root_depth, pm, win, w, h)?;
        Ok(Self {
            pm,
            width: w,
            height: h,
            depth: screen.root_depth,
            surface: None,
            cr: None,
        })
    }
    fn ensure_surface<'a>(&'a mut self, cx: &CairoXcb) -> Result<&'a Context> {
        if self.surface.is_none() {
            let drawable = XCBDrawable(self.pm);
            let surface = XCBSurface::create(
                &cx.cxcb_conn,
                &drawable,
                &cx.visual,
                self.width as i32,
                self.height as i32,
            )?;
            let cr = Context::new(&surface)?;
            self.surface = Some(surface);
            self.cr = Some(cr);
        }
        Ok(self.cr.as_ref().unwrap())
    }
    fn flush(&self) {
        if let Some(s) = &self.surface {
            s.flush();
        }
    }
    fn resize_if_needed(
        &mut self,
        conn: &XCBConnection,
        win: Window,
        w: u16,
        h: u16,
    ) -> Result<()> {
        if self.width == w && self.height == h {
            return Ok(());
        }
        conn.free_pixmap(self.pm)?;
        let pm = conn.generate_id()?;
        conn.create_pixmap(self.depth, pm, win, w, h)?;
        self.pm = pm;
        self.width = w;
        self.height = h;
        self.surface = None;
        self.cr = None;
        Ok(())
    }
    fn blit_to_window(&self, conn: &XCBConnection, win: Window, gc: Gcontext) -> Result<()> {
        conn.copy_area(self.pm, win, gc, 0, 0, 0, 0, self.width, self.height)?;
        Ok(())
    }
}

// ---------------- EWMH atoms（x11rb 版本） ----------------
struct Atoms {
    net_wm_window_type: Atom,
    net_wm_window_type_dock: Atom,
    net_wm_state: Atom,
    net_wm_state_above: Atom,
    net_wm_desktop: Atom,
    net_wm_strut_partial: Atom,
    net_wm_strut: Atom,
    net_wm_name: Atom,
    utf8_string: Atom,
}
fn intern_atoms(conn: &XCBConnection) -> Result<Atoms> {
    let intern = |name: &str| -> Result<Atom> {
        Ok(conn.intern_atom(false, name.as_bytes())?.reply()?.atom)
    };
    Ok(Atoms {
        net_wm_window_type: intern("_NET_WM_WINDOW_TYPE")?,
        net_wm_window_type_dock: intern("_NET_WM_WINDOW_TYPE_DOCK")?,
        net_wm_state: intern("_NET_WM_STATE")?,
        net_wm_state_above: intern("_NET_WM_STATE_ABOVE")?,
        net_wm_desktop: intern("_NET_WM_DESKTOP")?,
        net_wm_strut_partial: intern("_NET_WM_STRUT_PARTIAL")?,
        net_wm_strut: intern("_NET_WM_STRUT")?,
        net_wm_name: intern("_NET_WM_NAME")?,
        utf8_string: intern("UTF8_STRING")?,
    })
}
fn set_dock_properties(
    conn: &XCBConnection,
    atoms: &Atoms,
    screen: &Screen,
    win: Window,
    _w: u16,
    h: u16,
) -> Result<()> {
    conn.change_property32(
        PropMode::REPLACE,
        win,
        atoms.net_wm_window_type,
        AtomEnum::ATOM,
        &[atoms.net_wm_window_type_dock],
    )?;
    conn.change_property32(
        PropMode::REPLACE,
        win,
        atoms.net_wm_state,
        AtomEnum::ATOM,
        &[atoms.net_wm_state_above],
    )?;
    conn.change_property32(
        PropMode::REPLACE,
        win,
        atoms.net_wm_desktop,
        AtomEnum::CARDINAL,
        &[0xFFFFFFFF],
    )?;

    let sw = screen.width_in_pixels as u32;
    let top = h as u32;
    let top_start_x = 0u32;
    let top_end_x = (sw.saturating_sub(1)) as u32;
    let strut_partial = [0, 0, top, 0, 0, 0, 0, 0, top_start_x, top_end_x, 0, 0];
    conn.change_property32(
        PropMode::REPLACE,
        win,
        atoms.net_wm_strut_partial,
        AtomEnum::CARDINAL,
        &strut_partial,
    )?;
    let strut = [0u32, 0, top, 0];
    conn.change_property32(
        PropMode::REPLACE,
        win,
        atoms.net_wm_strut,
        AtomEnum::CARDINAL,
        &strut,
    )?;
    let title = b"x11rb_bar";
    conn.change_property8(
        PropMode::REPLACE,
        win,
        atoms.net_wm_name,
        atoms.utf8_string,
        title,
    )?;
    Ok(())
}

// ---------------- redraw ----------------
fn redraw(
    cairo_xcb: &CairoXcb,
    conn: &XCBConnection,
    back: &mut BackBuffer,
    win: Window,
    gc: Gcontext,
    width: u16,
    height: u16,
    colors: &xbar_core::Colors,
    state: &mut AppState,
    font: &FontDescription,
    cfg: &BarConfig,
) -> Result<()> {
    let cr = back.ensure_surface(cairo_xcb)?;
    draw_bar(cr, width, height, colors, state, font, cfg)?;
    back.flush();
    back.blit_to_window(conn, win, gc)?;
    conn.flush()?;
    Ok(())
}

// ---------------- 事件处理 ----------------
fn handle_x_event(
    event: x11rb::protocol::Event,
    cairo_xcb: &CairoXcb,
    conn: &XCBConnection,
    back: &mut BackBuffer,
    win: Window,
    gc: Gcontext,
    current_width: &mut u16,
    current_height: &mut u16,
    colors: &xbar_core::Colors,
    state: &mut AppState,
    font: &FontDescription,
    cfg: &BarConfig,
) -> Result<()> {
    match event {
        x11rb::protocol::Event::Expose(e) => {
            if e.count == 0 {
                back.blit_to_window(conn, win, gc)?;
                conn.flush()?;
            }
        }
        x11rb::protocol::Event::ConfigureNotify(e) => {
            if e.window == win {
                *current_width = e.width as u16;
                *current_height = e.height as u16;
                back.resize_if_needed(conn, win, *current_width, *current_height)?;
                redraw(
                    cairo_xcb,
                    conn,
                    back,
                    win,
                    gc,
                    *current_width,
                    *current_height,
                    colors,
                    state,
                    font,
                    cfg,
                )?;
            }
        }
        x11rb::protocol::Event::MotionNotify(e) => {
            let hovered = state.ss_rect.contains(e.event_x, e.event_y);
            if hovered != state.is_ss_hover {
                state.is_ss_hover = hovered;
                redraw(
                    cairo_xcb,
                    conn,
                    back,
                    win,
                    gc,
                    *current_width,
                    *current_height,
                    colors,
                    state,
                    font,
                    cfg,
                )?;
            }
        }
        x11rb::protocol::Event::ButtonPress(e) => {
            let px = e.event_x;
            let py = e.event_y;
            if state.handle_buttons(px, py, e.detail) {
                redraw(
                    cairo_xcb,
                    conn,
                    back,
                    win,
                    gc,
                    *current_width,
                    *current_height,
                    colors,
                    state,
                    font,
                    cfg,
                )?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn drain_x_events(
    cairo_xcb: &CairoXcb,
    conn: &XCBConnection,
    back: &mut BackBuffer,
    win: Window,
    gc: Gcontext,
    current_width: &mut u16,
    current_height: &mut u16,
    colors: &xbar_core::Colors,
    state: &mut AppState,
    font: &FontDescription,
    cfg: &BarConfig,
) -> Result<()> {
    while let Some(event) = conn.poll_for_event()? {
        handle_x_event(
            event,
            cairo_xcb,
            conn,
            back,
            win,
            gc,
            current_width,
            current_height,
            colors,
            state,
            font,
            cfg,
        )?;
    }
    Ok(())
}

// ---------------- 主程序 ----------------
fn main() -> Result<()> {
    // 参数
    let args: Vec<String> = env::args().collect();
    let shared_path = args.get(1).cloned().unwrap_or_default();

    // 日志
    if let Err(e) = initialize_logging("x11rb_bar", &shared_path) {
        eprintln!("Failed to initialize logging: {}", e);
        std::process::exit(1);
    }

    // 共享内存
    let shared_buffer = SharedRingBuffer::create_shared_ring_buffer_aux(&shared_path).map(Arc::new);
    let shared_efd = spawn_shared_eventfd_notifier(shared_buffer.clone(), true);

    // X 连接（xcb_ffi）
    let (conn, screen_num) = XCBConnection::connect(None)?;
    let screen = &conn.setup().roots[screen_num];

    // Cairo XCB 桥接对象
    let cairo_xcb = build_cairo_xcb_by_ffi(&conn, screen)?;

    // 配色与界面配置
    let colors = default_colors();
    let cfg = BarConfig {
        bar_height: 40,
        padding_x: 8.0,
        padding_y: 4.0,
        tag_spacing: 6.0,
        pill_hpadding: 10.0,
        pill_radius: 5.0, // 与原 x11rb_bar 一致
        shape_style: ShapeStyle::Pill,
        time_icon: "",
        screenshot_label: " Screenshot",
    };

    // 窗口 + GC
    let win = conn.generate_id()?;
    let gc = conn.generate_id()?;
    conn.create_gc(gc, screen.root, &CreateGCAux::new())?;

    let mut current_width = screen.width_in_pixels;
    let mut current_height = cfg.bar_height;
    conn.create_window(
        x11rb::COPY_FROM_PARENT as u8,
        win,
        screen.root,
        0,
        0,
        current_width,
        current_height,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new()
            .background_pixmap(x11rb::NONE)
            .event_mask(
                EventMask::EXPOSURE
                    | EventMask::STRUCTURE_NOTIFY
                    | EventMask::BUTTON_PRESS
                    | EventMask::BUTTON_RELEASE
                    | EventMask::POINTER_MOTION
                    | EventMask::ENTER_WINDOW
                    | EventMask::LEAVE_WINDOW,
            ),
    )?;
    let atoms = intern_atoms(&conn)?;
    set_dock_properties(&conn, &atoms, screen, win, current_width, current_height)?;
    conn.map_window(win)?;
    conn.flush()?;

    // 再次明确背景不自动清空
    conn.change_window_attributes(
        win,
        &ChangeWindowAttributesAux::new().background_pixmap(x11rb::NONE),
    )?;

    // 字体
    let font = FontDescription::from_string("JetBrainsMono Nerd Font 11");

    // 后备缓冲
    let mut back = BackBuffer::new(&conn, screen, win, current_width, current_height)?;

    // 状态
    let mut state = AppState::new(shared_buffer);

    // 首次绘制
    redraw(
        &cairo_xcb,
        &conn,
        &mut back,
        win,
        gc,
        current_width,
        current_height,
        &colors,
        &mut state,
        &font,
        &cfg,
    )?;

    // ============ epoll + timerfd ============
    let x_fd = conn.as_fd().as_raw_fd();

    let epfd = unsafe { libc::epoll_create1(libc::EPOLL_CLOEXEC) };
    if epfd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }

    let tfd = unsafe {
        libc::timerfd_create(
            libc::CLOCK_MONOTONIC,
            libc::TFD_NONBLOCK | libc::TFD_CLOEXEC,
        )
    };
    if tfd < 0 {
        let e = std::io::Error::last_os_error();
        unsafe { libc::close(epfd) };
        return Err(e.into());
    }
    arm_second_timer(tfd).map_err(|e| anyhow::anyhow!("arm_second_timer failed: {}", e))?;

    const X_TOKEN: u64 = 1;
    const TFD_TOKEN: u64 = 2;
    const SHARED_TOKEN: u64 = xbar_core::SHARED_TOKEN;

    let mut ev_x = libc::epoll_event {
        events: libc::EPOLLIN as u32,
        u64: X_TOKEN,
    };
    let rc = unsafe { libc::epoll_ctl(epfd, libc::EPOLL_CTL_ADD, x_fd, &mut ev_x as *mut _) };
    if rc < 0 {
        let e = std::io::Error::last_os_error();
        unsafe {
            libc::close(tfd);
            libc::close(epfd);
        }
        return Err(e.into());
    }

    let mut ev_t = libc::epoll_event {
        events: libc::EPOLLIN as u32,
        u64: TFD_TOKEN,
    };
    let rc = unsafe { libc::epoll_ctl(epfd, libc::EPOLL_CTL_ADD, tfd, &mut ev_t as *mut _) };
    if rc < 0 {
        let e = std::io::Error::last_os_error();
        unsafe {
            libc::close(tfd);
            libc::close(epfd);
        }
        return Err(e.into());
    }

    if let Some(efd) = shared_efd {
        let mut ev_s = libc::epoll_event {
            events: libc::EPOLLIN as u32,
            u64: SHARED_TOKEN,
        };
        let rc = unsafe { libc::epoll_ctl(epfd, libc::EPOLL_CTL_ADD, efd, &mut ev_s as *mut _) };
        if rc < 0 {
            let e = std::io::Error::last_os_error();
            unsafe {
                libc::close(tfd);
                libc::close(epfd);
                libc::close(efd);
            }
            return Err(e.into());
        }
    }

    const EP_EVENTS_CAP: usize = 32;
    let mut events: [libc::epoll_event; EP_EVENTS_CAP] =
        unsafe { MaybeUninit::zeroed().assume_init() };

    let periodic_tick = |state: &mut AppState| -> Result<bool> {
        let mut need_redraw = false;
        if state.last_clock_update.elapsed() >= Duration::from_secs(1) {
            state.last_clock_update = Instant::now();
            need_redraw = true;
        }
        if state.last_monitor_update.elapsed() >= Duration::from_secs(2) {
            state.system_monitor.update_if_needed();
            state.audio_manager.update_if_needed();
            state.last_monitor_update = Instant::now();
            need_redraw = true;
        }
        Ok(need_redraw)
    };

    loop {
        let nfds = loop {
            let n = unsafe {
                libc::epoll_wait(
                    epfd,
                    events.as_mut_ptr(),
                    EP_EVENTS_CAP as i32,
                    -1, // 阻塞
                )
            };
            if n >= 0 {
                break n;
            }
            let err = std::io::Error::last_os_error();
            if let Some(code) = err.raw_os_error() {
                if code == libc::EINTR {
                    continue;
                }
            }
            warn!("[main] epoll_wait failed: {}", err);
            thread::sleep(Duration::from_millis(10));
            break 0;
        };

        for i in 0..(nfds as usize) {
            let ev = events[i];
            match ev.u64 {
                X_TOKEN => {
                    drain_x_events(
                        &cairo_xcb,
                        &conn,
                        &mut back,
                        win,
                        gc,
                        &mut current_width,
                        &mut current_height,
                        &colors,
                        &mut state,
                        &font,
                        &cfg,
                    )?;
                }
                TFD_TOKEN => {
                    let mut buf = [0u8; 8];
                    loop {
                        let r = unsafe {
                            libc::read(tfd, buf.as_mut_ptr() as *mut libc::c_void, buf.len())
                        };
                        if r == 8 {
                            if periodic_tick(&mut state)? {
                                redraw(
                                    &cairo_xcb,
                                    &conn,
                                    &mut back,
                                    win,
                                    gc,
                                    current_width,
                                    current_height,
                                    &colors,
                                    &mut state,
                                    &font,
                                    &cfg,
                                )?;
                            }
                            continue;
                        } else if r < 0 {
                            let err = std::io::Error::last_os_error();
                            if let Some(code) = err.raw_os_error() {
                                if code == libc::EAGAIN || code == libc::EWOULDBLOCK {
                                    break;
                                }
                            }
                            warn!("[main] timerfd read error: {}", err);
                            break;
                        } else {
                            break;
                        }
                    }
                }
                SHARED_TOKEN => {
                    if let Some(efd) = shared_efd {
                        // 抽干 eventfd
                        let mut buf8 = [0u8; 8];
                        loop {
                            let r = unsafe {
                                libc::read(efd, buf8.as_mut_ptr() as *mut libc::c_void, buf8.len())
                            };
                            if r == 8 {
                                continue;
                            } else if r < 0 {
                                let err = std::io::Error::last_os_error();
                                if let Some(code) = err.raw_os_error() {
                                    if code == libc::EAGAIN || code == libc::EWOULDBLOCK {
                                        break;
                                    }
                                }
                                warn!("[main] eventfd read error: {}", err);
                                break;
                            } else {
                                break;
                            }
                        }

                        // 读取共享消息（保留最后一条）
                        let mut need_redraw = false;
                        if let Some(buf_arc) = state.shared_buffer.as_ref().cloned() {
                            match buf_arc.try_read_latest_message() {
                                Ok(Some(msg)) => {
                                    log::trace!("redraw by msg: {:?}", msg);
                                    state.update_from_shared(msg);
                                    need_redraw = true;
                                }
                                Ok(None) => { /* 没有消息 */ }
                                Err(e) => {
                                    warn!("Shared try_read_latest_message failed: {}", e);
                                }
                            }
                        }
                        if need_redraw {
                            redraw(
                                &cairo_xcb,
                                &conn,
                                &mut back,
                                win,
                                gc,
                                current_width,
                                current_height,
                                &colors,
                                &mut state,
                                &font,
                                &cfg,
                            )?;
                        }
                    }
                }
                other => {
                    debug!("[main] unexpected epoll token: {}", other);
                }
            }
        }
    }
}
