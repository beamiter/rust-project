use anyhow::Result;
use cairo::ffi::{xcb_connection_t, xcb_visualtype_t};
use cairo::{Context, XCBConnection as CairoXCBConnection, XCBDrawable, XCBSurface, XCBVisualType};
use log::{debug, warn};
use pango::FontDescription;
use shared_structures::{SharedMessage, SharedRingBuffer};
use std::env;
use std::mem::MaybeUninit;
use std::os::fd::AsRawFd as _;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use xbar_core::{
    AppState, BarConfig, ShapeStyle, arm_second_timer, default_colors, draw_bar,
    initialize_logging, spawn_shared_eventfd_notifier,
};

use libc;

use xcb::{self, Xid, x};

// ---------------- Cairo XCB 桥接（xcb 版本） ----------------
struct CairoXcb {
    cxcb_conn: CairoXCBConnection,
    visual: XCBVisualType,
    _visual_owner: Box<x::Visualtype>,
}
fn find_visual_by_id_and_depth(
    screen: &x::Screen,
    target_visual_id: u32,
    target_depth: u8,
) -> Option<x::Visualtype> {
    for depth in screen.allowed_depths() {
        if depth.depth() == target_depth {
            for visual in depth.visuals() {
                if visual.visual_id() == target_visual_id {
                    return Some(visual.clone());
                }
            }
        }
    }
    None
}
fn build_cairo_xcb(conn: &xcb::Connection, screen: &x::Screen) -> Result<CairoXcb> {
    let root_visual_id = screen.root_visual();
    let root_depth = screen.root_depth();
    let visual = find_visual_by_id_and_depth(screen, root_visual_id, root_depth)
        .ok_or_else(|| anyhow::anyhow!("Could not find visualtype for root_visual"))?;
    let visual_owner = Box::new(visual);
    let ptr = (&*visual_owner) as *const x::Visualtype as *mut xcb_visualtype_t;
    let cxcb_vis = unsafe { XCBVisualType::from_raw_none(ptr) };
    let raw = conn.get_raw_conn();
    let cxcb_conn = unsafe { CairoXCBConnection::from_raw_none(raw as *mut xcb_connection_t) };
    Ok(CairoXcb {
        cxcb_conn,
        visual: cxcb_vis,
        _visual_owner: visual_owner,
    })
}

// ---------------- 后备缓冲（xcb 版本） ----------------
struct BackBuffer {
    pm: x::Pixmap,
    width: u16,
    height: u16,
    depth: u8,
    surface: Option<XCBSurface>,
    cr: Option<Context>,
}
impl BackBuffer {
    fn new(
        conn: &xcb::Connection,
        screen: &x::Screen,
        win: x::Window,
        w: u16,
        h: u16,
    ) -> Result<Self> {
        let pm = conn.generate_id();
        conn.send_and_check_request(&x::CreatePixmap {
            depth: screen.root_depth(),
            pid: pm,
            drawable: x::Drawable::Window(win),
            width: w,
            height: h,
        })?;
        Ok(Self {
            pm,
            width: w,
            height: h,
            depth: screen.root_depth(),
            surface: None,
            cr: None,
        })
    }
    fn ensure_surface<'a>(&'a mut self, cx: &CairoXcb) -> Result<&'a Context> {
        if self.surface.is_none() {
            let drawable = XCBDrawable(self.pm.resource_id());
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
        conn: &xcb::Connection,
        win: x::Window,
        w: u16,
        h: u16,
    ) -> Result<()> {
        if self.width == w && self.height == h {
            return Ok(());
        }
        conn.send_and_check_request(&x::FreePixmap { pixmap: self.pm })?;
        let pm = conn.generate_id();
        conn.send_and_check_request(&x::CreatePixmap {
            depth: self.depth,
            pid: pm,
            drawable: x::Drawable::Window(win),
            width: w,
            height: h,
        })?;
        self.pm = pm;
        self.width = w;
        self.height = h;
        self.surface = None;
        self.cr = None;
        Ok(())
    }
    fn blit_to_window(
        &self,
        conn: &xcb::Connection,
        win: x::Window,
        gc: x::Gcontext,
    ) -> Result<()> {
        conn.send_and_check_request(&x::CopyArea {
            src_drawable: x::Drawable::Pixmap(self.pm),
            dst_drawable: x::Drawable::Window(win),
            gc,
            src_x: 0,
            src_y: 0,
            dst_x: 0,
            dst_y: 0,
            width: self.width,
            height: self.height,
        })?;
        Ok(())
    }
}

// ---------------- EWMH atoms（xcb 版本） ----------------
struct Atoms {
    net_wm_window_type: x::Atom,
    net_wm_window_type_dock: x::Atom,
    net_wm_state: x::Atom,
    net_wm_state_above: x::Atom,
    net_wm_desktop: x::Atom,
    net_wm_strut_partial: x::Atom,
    net_wm_strut: x::Atom,
    net_wm_name: x::Atom,
    utf8_string: x::Atom,
    atom: x::Atom,
    cardinal: x::Atom,
}

fn intern_atom(conn: &xcb::Connection, name: &str) -> Result<x::Atom> {
    let ck = conn.send_request(&x::InternAtom {
        only_if_exists: false,
        name: name.as_bytes(),
    });
    let reply = conn.wait_for_reply(ck)?;
    Ok(reply.atom())
}

fn intern_atoms(conn: &xcb::Connection) -> Result<Atoms> {
    Ok(Atoms {
        net_wm_window_type: intern_atom(conn, "_NET_WM_WINDOW_TYPE")?,
        net_wm_window_type_dock: intern_atom(conn, "_NET_WM_WINDOW_TYPE_DOCK")?,
        net_wm_state: intern_atom(conn, "_NET_WM_STATE")?,
        net_wm_state_above: intern_atom(conn, "_NET_WM_STATE_ABOVE")?,
        net_wm_desktop: intern_atom(conn, "_NET_WM_DESKTOP")?,
        net_wm_strut_partial: intern_atom(conn, "_NET_WM_STRUT_PARTIAL")?,
        net_wm_strut: intern_atom(conn, "_NET_WM_STRUT")?,
        net_wm_name: intern_atom(conn, "_NET_WM_NAME")?,
        utf8_string: intern_atom(conn, "UTF8_STRING")?,
        atom: intern_atom(conn, "ATOM")?,
        cardinal: intern_atom(conn, "CARDINAL")?,
    })
}

fn change_property_32(
    conn: &xcb::Connection,
    win: x::Window,
    property: x::Atom,
    r#type: x::Atom,
    data: &[u32],
) -> Result<()> {
    let mut bytes = Vec::with_capacity(data.len() * 4);
    for v in data {
        bytes.extend_from_slice(&v.to_ne_bytes());
    }
    conn.send_and_check_request(&x::ChangeProperty {
        mode: x::PropMode::Replace,
        window: win,
        property,
        r#type,
        data: &bytes,
    })?;
    Ok(())
}

fn change_property_8(
    conn: &xcb::Connection,
    win: x::Window,
    property: x::Atom,
    r#type: x::Atom,
    data: &[u8],
) -> Result<()> {
    conn.send_and_check_request(&x::ChangeProperty {
        mode: x::PropMode::Replace,
        window: win,
        property,
        r#type,
        data,
    })?;
    Ok(())
}

fn set_dock_properties(
    conn: &xcb::Connection,
    atoms: &Atoms,
    screen: &x::Screen,
    win: x::Window,
    _w: u16,
    h: u16,
) -> Result<()> {
    change_property_32(
        conn,
        win,
        atoms.net_wm_window_type,
        atoms.atom,
        &[atoms.net_wm_window_type_dock.resource_id()],
    )?;
    change_property_32(
        conn,
        win,
        atoms.net_wm_state,
        atoms.atom,
        &[atoms.net_wm_state_above.resource_id()],
    )?;
    change_property_32(
        conn,
        win,
        atoms.net_wm_desktop,
        atoms.cardinal,
        &[0xFFFFFFFF],
    )?;

    let sw = screen.width_in_pixels() as u32;
    let top = h as u32;
    let top_start_x = 0u32;
    let top_end_x = (sw.saturating_sub(1)) as u32;
    let strut_partial = [0, 0, top, 0, 0, 0, 0, 0, top_start_x, top_end_x, 0, 0];
    change_property_32(
        conn,
        win,
        atoms.net_wm_strut_partial,
        atoms.cardinal,
        &strut_partial,
    )?;
    let strut = [0u32, 0, top, 0];
    change_property_32(conn, win, atoms.net_wm_strut, atoms.cardinal, &strut)?;

    change_property_8(conn, win, atoms.net_wm_name, atoms.utf8_string, b"xcb_bar")?;
    Ok(())
}

// ---------------- redraw ----------------
fn redraw(
    cairo_xcb: &CairoXcb,
    conn: &xcb::Connection,
    back: &mut BackBuffer,
    win: x::Window,
    gc: x::Gcontext,
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
    event: xcb::Event,
    cairo_xcb: &CairoXcb,
    conn: &xcb::Connection,
    back: &mut BackBuffer,
    win: x::Window,
    gc: x::Gcontext,
    current_width: &mut u16,
    current_height: &mut u16,
    colors: &xbar_core::Colors,
    state: &mut AppState,
    font: &FontDescription,
    cfg: &BarConfig,
) -> Result<()> {
    match event {
        xcb::Event::X(x::Event::Expose(e)) => {
            if e.count() == 0 {
                back.blit_to_window(conn, win, gc)?;
                conn.flush()?;
            }
        }
        xcb::Event::X(x::Event::ConfigureNotify(e)) => {
            if e.window() == win {
                *current_width = e.width() as u16;
                *current_height = e.height() as u16;
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
        xcb::Event::X(x::Event::MotionNotify(e)) => {
            let hovered = state.ss_rect.contains(e.event_x(), e.event_y());
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
        xcb::Event::X(x::Event::ButtonPress(e)) => {
            let px = e.event_x();
            let py = e.event_y();
            let button: u8 = e.detail().into();
            if state.handle_buttons(px, py, button) {
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
    conn: &xcb::Connection,
    back: &mut BackBuffer,
    win: x::Window,
    gc: x::Gcontext,
    current_width: &mut u16,
    current_height: &mut u16,
    colors: &xbar_core::Colors,
    state: &mut AppState,
    font: &FontDescription,
    cfg: &BarConfig,
) -> Result<()> {
    while let Ok(Some(event)) = conn.poll_for_event() {
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
    if let Err(e) = initialize_logging("xcb_bar", &shared_path) {
        eprintln!("Failed to initialize logging: {}", e);
        std::process::exit(1);
    }

    // 共享内存
    let shared_buffer = SharedRingBuffer::create_shared_ring_buffer_aux(&shared_path).map(Arc::new);
    let shared_efd = spawn_shared_eventfd_notifier(shared_buffer.clone(), true);

    // X 连接
    let (conn, screen_num) = xcb::Connection::connect(None)?;
    let setup = conn.get_setup();
    let screen = setup
        .roots()
        .nth(screen_num as usize)
        .ok_or_else(|| anyhow::anyhow!("No screen found"))?;

    // Cairo XCB 桥接
    let cairo_xcb = build_cairo_xcb(&conn, &screen)?;

    // 配色与界面配置
    let colors = default_colors();
    let cfg = BarConfig {
        bar_height: 40,
        padding_x: 8.0,
        padding_y: 4.0,
        tag_spacing: 6.0,
        pill_hpadding: 10.0,
        pill_radius: 8.0, // 与原 xcb_bar 一致
        shape_style: ShapeStyle::Pill,
        time_icon: "",
        screenshot_label: " Screenshot",
    };

    // 窗口 + GC
    let win = conn.generate_id();
    let gc = conn.generate_id();
    conn.send_and_check_request(&x::CreateGc {
        cid: gc,
        drawable: x::Drawable::Window(screen.root()),
        value_list: &[],
    })?;

    let mut current_width = screen.width_in_pixels();
    let mut current_height = cfg.bar_height;

    // 创建窗口，背景 None，设置事件掩码
    let cw_values = &[
        x::Cw::BackPixmap(x::Pixmap::none()),
        x::Cw::EventMask(
            x::EventMask::EXPOSURE
                | x::EventMask::STRUCTURE_NOTIFY
                | x::EventMask::BUTTON_PRESS
                | x::EventMask::BUTTON_RELEASE
                | x::EventMask::POINTER_MOTION
                | x::EventMask::ENTER_WINDOW
                | x::EventMask::LEAVE_WINDOW,
        ),
    ];
    conn.send_and_check_request(&x::CreateWindow {
        depth: x::COPY_FROM_PARENT as u8,
        wid: win,
        parent: screen.root(),
        x: 0,
        y: 0,
        width: current_width,
        height: current_height,
        border_width: 0,
        class: x::WindowClass::InputOutput,
        visual: screen.root_visual(),
        value_list: cw_values,
    })?;

    let atoms = intern_atoms(&conn)?;
    set_dock_properties(&conn, &atoms, &screen, win, current_width, current_height)?;
    conn.send_and_check_request(&x::MapWindow { window: win })?;
    conn.flush()?;

    // 明确背景不自动清空
    conn.send_and_check_request(&x::ChangeWindowAttributes {
        window: win,
        value_list: &[x::Cw::BackPixmap(x::Pixmap::none())],
    })?;

    // 字体
    let font = FontDescription::from_string("JetBrainsMono Nerd Font 11");

    // 后备缓冲
    let mut back = BackBuffer::new(&conn, &screen, win, current_width, current_height)?;

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

    // epoll + timerfd
    let x_fd = conn.as_raw_fd();

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
            let n =
                unsafe { libc::epoll_wait(epfd, events.as_mut_ptr(), EP_EVENTS_CAP as i32, -1) };
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

                        let mut need_redraw = false;
                        if let Some(buf_arc) = state.shared_buffer.as_ref().cloned() {
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
                                state.update_from_shared(msg);
                                need_redraw = true;
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
