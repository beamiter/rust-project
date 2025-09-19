use anyhow::Result;
use cairo::ffi::{xcb_connection_t, xcb_visualtype_t};
use chrono::Local;
use log::{debug, error, info, warn};
use pangocairo::functions::{create_layout, show_layout};
use std::env;
use std::os::fd::{AsFd, AsRawFd};
use std::thread;
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::wrapper::ConnectionExt as _;

use std::{mem::MaybeUninit, sync::Arc};

use x11rb::protocol::xproto::{CreateGCAux, CreateWindowAux, EventMask, WindowClass};
use x11rb::xcb_ffi::XCBConnection;

use libc;

// Cairo/Pango
use cairo::{Context, XCBConnection as CairoXCBConnection, XCBDrawable, XCBSurface, XCBVisualType};
use pango::FontDescription;

use std::f64::consts::{FRAC_PI_2, PI};
use xcb;

#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
enum ShapeStyle {
    Chamfer,
    Pill,
}

// ---------------- 外部模块 ----------------
pub mod audio_manager;
use audio_manager::AudioManager;

pub mod error;
use error::AppError;

pub mod system_monitor;
use system_monitor::SystemMonitor;

use shared_structures::{CommandType, MonitorInfo, SharedCommand, SharedMessage, SharedRingBuffer};

// ---------------- 日志初始化 ----------------
use chrono::Local as ChronoLocal;
use flexi_logger::{Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming};

fn initialize_logging(shared_path: &str) -> Result<(), AppError> {
    let tmp_now = ChronoLocal::now();
    let timestamp = tmp_now.format("%Y-%m-%d_%H_%M_%S").to_string();

    let log_dir_candidates = [Some("/var/tmp/jwm".to_string())];

    let log_dir = log_dir_candidates
        .into_iter()
        .flatten()
        .find(|p| {
            std::fs::create_dir_all(p).ok();
            std::fs::metadata(p).map(|m| m.is_dir()).unwrap_or(false)
        })
        .unwrap_or_else(|| ".".to_string());

    let file_name = if shared_path.is_empty() {
        "x11rb_bar".to_string()
    } else {
        std::path::Path::new(shared_path)
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| format!("x11rb_bar_{}", name))
            .unwrap_or_else(|| "x11rb_bar".to_string())
    };

    let log_filename = format!("{}_{}", file_name, timestamp);
    let log_spec = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());

    Logger::try_with_str(log_spec)
        .map_err(|e| AppError::config(format!("Failed to create logger: {}", e)))?
        .format_for_files(flexi_logger::detailed_format)
        .format_for_stderr(flexi_logger::colored_opt_format)
        .log_to_file(
            FileSpec::default()
                .directory(&log_dir)
                .basename(log_filename)
                .suffix("log"),
        )
        .duplicate_to_stdout(Duplicate::Info)
        .rotate(
            Criterion::Size(10_000_000), // 10MB
            Naming::Numbers,
            Cleanup::KeepLogFiles(5),
        )
        .start()
        .map_err(|e| AppError::config(format!("Failed to start logger: {}", e)))?;

    info!("Log directory: {}", log_dir);
    Ok(())
}

// ---------------- X11 / 绘图基础 ----------------
const BAR_HEIGHT: u16 = 40;
const PADDING_X: f64 = 8.0;
const PADDING_Y: f64 = 4.0;
const TAG_SPACING: f64 = 6.0;
const PILL_HPADDING: f64 = 10.0;
const PILL_RADIUS: f64 = 5.0;

// Cairo 颜色
#[derive(Clone, Copy, Debug)]
struct Color {
    r: f64,
    g: f64,
    b: f64,
}
impl Color {
    fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self {
            r: r as f64 / 255.0,
            g: g as f64 / 255.0,
            b: b as f64 / 255.0,
        }
    }
}
#[derive(Clone)]
#[allow(dead_code)]
struct Colors {
    bg: Color,
    text: Color,
    white: Color,
    black: Color,
    tag_colors: [Color; 9],
    gray: Color,
    red: Color,
    green: Color,
    yellow: Color,
    orange: Color,
    blue: Color,
    purple: Color,
    teal: Color,
    time: Color,
}

#[derive(Clone, Copy, Debug, Default)]
struct Rect {
    x: i16,
    y: i16,
    w: u16,
    h: u16,
}
impl Rect {
    fn contains(&self, px: i16, py: i16) -> bool {
        px >= self.x
            && py >= self.y
            && (px as i32) < (self.x as i32 + self.w as i32)
            && (py as i32) < (self.y as i32 + self.h as i32)
    }
}

// ========== Cairo XCB 桥接 ==========
// 持有 Visualtype 的所有权，避免内存泄漏/悬垂
struct CairoXcb {
    cxcb_conn: CairoXCBConnection,
    visual: XCBVisualType,                  // 非拥有指针包装
    _visual_owner: Box<xcb::x::Visualtype>, // 真正所有权，延长生命周期
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
    info!(
        "Found visual id={} class={:?}",
        visual.visual_id, visual.class,
    );
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

// ---------------- 文本绘制（Pango）与形状（Cairo） ----------------
fn pango_text_size(cr: &Context, font: &FontDescription, text: &str) -> (i32, i32) {
    let layout = create_layout(cr);
    layout.set_font_description(Some(font));
    layout.set_text(text);
    layout.pixel_size()
}
fn pango_draw_text_centered(
    cr: &Context,
    font: &FontDescription,
    color: Color,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    text: &str,
) {
    let layout = create_layout(cr);
    layout.set_font_description(Some(font));
    layout.set_text(text);
    let (tw, th) = layout.pixel_size();
    let tx = x + (w - tw as f64) / 2.0;
    let ty = y + (h - th as f64) / 2.0 - 1.0;
    cr.set_source_rgb(color.r, color.g, color.b);
    cr.move_to(tx, ty);
    show_layout(cr, &layout);
}
fn pango_draw_text_left(
    cr: &Context,
    font: &FontDescription,
    color: Color,
    x: f64,
    y: f64,
    text: &str,
) {
    let layout = create_layout(cr);
    layout.set_font_description(Some(font));
    layout.set_text(text);
    cr.set_source_rgb(color.r, color.g, color.b);
    cr.move_to(x, y);
    show_layout(cr, &layout);
}

fn cairo_path_round_rect(cr: &Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    cr.new_path();
    // 从上边开始，顺时针连四个弧形
    cr.move_to(x + r, y);
    cr.line_to(x + w - r, y);
    cr.arc(x + w - r, y + r, r, -FRAC_PI_2, 0.0);
    cr.line_to(x + w, y + h - r);
    cr.arc(x + w - r, y + h - r, r, 0.0, FRAC_PI_2);
    cr.line_to(x + r, y + h);
    cr.arc(x + r, y + h - r, r, FRAC_PI_2, PI);
    cr.line_to(x, y + r);
    cr.arc(x + r, y + r, r, PI, 3.0 * FRAC_PI_2);
    cr.close_path();
}

fn fill_round(cr: &Context, x: f64, y: f64, w: f64, h: f64, r: f64, color: Color) -> Result<()> {
    cairo_path_round_rect(cr, x, y, w, h, r);
    cr.set_source_rgb(color.r, color.g, color.b);
    cr.fill()
        .map_err(|e| anyhow::anyhow!("cairo fill failed: {:?}", e))
}

fn stroke_round_with_fill(
    cr: &Context,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    r: f64,
    border_w: f64,
    border_color: Color,
    fill_color: Option<Color>,
) -> Result<()> {
    if border_w <= 0.0 {
        if let Some(fill) = fill_color {
            fill_round(cr, x, y, w, h, r, fill)?;
        }
        return Ok(());
    }
    // 外边框
    fill_round(cr, x, y, w, h, r, border_color)?;
    // 内填充
    if let Some(fill) = fill_color {
        let x2 = x + border_w;
        let y2 = y + border_w;
        let w2 = (w - 2.0 * border_w).max(0.0);
        let h2 = (h - 2.0 * border_w).max(0.0);
        if w2 > 0.0 && h2 > 0.0 {
            let r2 = (r - border_w).max(0.0);
            fill_round(cr, x2, y2, w2, h2, r2, fill)?;
        }
    }
    Ok(())
}

/// 统一的“按钮形状绘制”封装：根据样式选择 chamfer 或 pill（圆角胶囊）
fn stroke_shape_with_fill(
    cr: &Context,
    style: ShapeStyle,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    k: f64,
    border_w: f64,
    border_color: Color,
    fill_color: Option<Color>,
) -> Result<()> {
    match style {
        ShapeStyle::Chamfer => {
            stroke_chamfer_with_fill(cr, x, y, w, h, k, border_w, border_color, fill_color)
        }
        ShapeStyle::Pill => {
            let r = k.min(h / 2.0).floor();
            stroke_round_with_fill(cr, x, y, w, h, r, border_w, border_color, fill_color)
        }
    }
}

fn cairo_path_chamfer(cr: &Context, x: f64, y: f64, w: f64, h: f64, k: f64) {
    let k = k.min(w / 2.0).min(h / 2.0).max(0.0);
    cr.new_path();
    cr.move_to(x + k, y);
    cr.line_to(x + w - k, y);
    cr.line_to(x + w, y + k);
    cr.line_to(x + w, y + h - k);
    cr.line_to(x + w - k, y + h);
    cr.line_to(x + k, y + h);
    cr.line_to(x, y + h - k);
    cr.line_to(x, y + k);
    cr.close_path();
}
fn fill_chamfer(cr: &Context, x: f64, y: f64, w: f64, h: f64, k: f64, color: Color) -> Result<()> {
    cairo_path_chamfer(cr, x, y, w, h, k);
    cr.set_source_rgb(color.r, color.g, color.b);
    cr.fill()
        .map_err(|e| anyhow::anyhow!("cairo fill failed: {:?}", e))
}
fn stroke_chamfer_with_fill(
    cr: &Context,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    k: f64,
    border_w: f64,
    border_color: Color,
    fill_color: Option<Color>,
) -> Result<()> {
    if border_w <= 0.0 {
        if let Some(fill) = fill_color {
            fill_chamfer(cr, x, y, w, h, k, fill)?;
        }
        return Ok(());
    }
    // 外边框
    fill_chamfer(cr, x, y, w, h, k, border_color)?;
    // 内部填充
    if let Some(fill) = fill_color {
        let x2 = x + border_w;
        let y2 = y + border_w;
        let w2 = (w - 2.0 * border_w).max(0.0);
        let h2 = (h - 2.0 * border_w).max(0.0);
        if w2 > 0.0 && h2 > 0.0 {
            let k2 = (k - border_w).max(0.0);
            fill_chamfer(cr, x2, y2, w2, h2, k2, fill)?;
        }
    }
    Ok(())
}

// usage 颜色映射
fn usage_bg_color(colors: &Colors, usage: f32) -> Color {
    let u = usage.clamp(0.0, 100.0);
    if u <= 30.0 {
        colors.green
    } else if u <= 60.0 {
        colors.yellow
    } else if u <= 80.0 {
        colors.orange
    } else {
        colors.red
    }
}
fn usage_text_color(colors: &Colors, usage: f32) -> Color {
    if usage <= 60.0 {
        colors.black
    } else {
        colors.white
    }
}

// tag 样式
fn tag_visuals(
    colors: &Colors,
    mi: Option<&MonitorInfo>,
    idx: usize,
) -> (Color, f64, Color, Color, bool) {
    let tag_color = colors.tag_colors[idx.min(colors.tag_colors.len() - 1)];
    if let Some(monitor) = mi {
        if let Some(status) = monitor.tag_status_vec.get(idx) {
            if status.is_urg {
                return (colors.red, 2.0, colors.red, colors.white, true);
            } else if status.is_selected {
                return (tag_color, 2.0, tag_color, colors.black, true);
            } else if status.is_filled {
                return (tag_color, 1.0, tag_color, colors.black, true);
            } else if status.is_occ {
                return (colors.gray, 1.0, colors.gray, colors.white, true);
            }
        }
    }
    (colors.bg, 1.0, colors.gray, colors.gray, true)
}

// ---------------- EWMH atoms ----------------
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

// ---------------- 应用状态 ----------------
struct AppState {
    shared_buffer: Option<Arc<SharedRingBuffer>>,
    monitor_info: Option<MonitorInfo>,
    monitor_num: i32,
    layout_symbol: String,

    tag_rects: [Rect; 9],
    active_tab: usize,

    layout_button_rect: Rect,
    layout_selector_open: bool,
    layout_option_rects: [Rect; 3],

    ss_rect: Rect,
    time_rect: Rect,
    is_ss_hover: bool,
    show_seconds: bool,

    audio_manager: AudioManager,
    system_monitor: SystemMonitor,

    last_clock_update: Instant,
    last_monitor_update: Instant,

    shape_style: ShapeStyle,
}
impl AppState {
    fn new(shared_buffer: Option<Arc<SharedRingBuffer>>) -> Self {
        Self {
            shared_buffer,
            monitor_info: None,
            monitor_num: 0,
            layout_symbol: "[]=".to_string(),
            tag_rects: [Rect::default(); 9],
            active_tab: 0,

            layout_button_rect: Rect::default(),
            layout_selector_open: false,
            layout_option_rects: [Rect::default(), Rect::default(), Rect::default()],

            ss_rect: Rect::default(),
            time_rect: Rect::default(),
            is_ss_hover: false,
            show_seconds: false,

            audio_manager: AudioManager::new(),
            system_monitor: SystemMonitor::new(5),

            last_clock_update: Instant::now(),
            last_monitor_update: Instant::now(),

            shape_style: ShapeStyle::Pill,
        }
    }
    fn monitor_num_to_label(num: i32) -> String {
        format!("M{}", num)
    }
    fn update_from_shared(&mut self, msg: SharedMessage) {
        debug!("SharedMemoryUpdated: {:?}", msg.timestamp);
        self.monitor_info = Some(msg.monitor_info);
        if let Some(mi) = self.monitor_info.as_ref() {
            self.layout_symbol = mi.get_ltsymbol();
            self.monitor_num = mi.monitor_num;
            for (i, tag) in mi.tag_status_vec.iter().enumerate() {
                if tag.is_selected {
                    self.active_tab = i;
                }
            }
        }
    }
    fn send_tag_command_index(&mut self, idx: usize, is_view: bool) {
        let tag_bit = 1 << idx;
        let cmd = if is_view {
            SharedCommand::view_tag(tag_bit, self.monitor_num)
        } else {
            SharedCommand::toggle_tag(tag_bit, self.monitor_num)
        };
        if let Some(buf) = &self.shared_buffer {
            match buf.send_command(cmd) {
                Ok(true) => info!("Sent command: {:?} by shared_buffer", cmd),
                Ok(false) => warn!("Command buffer full, command dropped"),
                Err(e) => error!("Failed to send command: {}", e),
            }
        }
    }
    fn send_layout_command(&mut self, layout_index: u32) {
        let cmd = SharedCommand::new(CommandType::SetLayout, layout_index, self.monitor_num);
        if let Some(buf) = &self.shared_buffer {
            match buf.send_command(cmd) {
                Ok(true) => info!("Sent command: {:?} by shared_buffer", cmd),
                Ok(false) => warn!("Command buffer full, command dropped"),
                Err(e) => error!("Failed to send command: {}", e),
            }
        }
    }
    fn format_time(&self) -> String {
        let now = Local::now();
        if self.show_seconds {
            now.format("%Y-%m-%d %H:%M:%S").to_string()
        } else {
            now.format("%Y-%m-%d %H:%M").to_string()
        }
    }
}

// ---------------- 后备缓冲（Pixmap + 复用 Cairo Surface/Context） ----------------
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
        // 尺寸改变，旧 surface/context 作废
        self.surface = None;
        self.cr = None;
        Ok(())
    }
    fn blit_to_window(&self, conn: &XCBConnection, win: Window, gc: Gcontext) -> Result<()> {
        conn.copy_area(self.pm, win, gc, 0, 0, 0, 0, self.width, self.height)?;
        Ok(())
    }
}

// ---------------- 绘制 bar（使用复用的 Context） ----------------
fn draw_bar(
    cr: &Context,
    width: u16,
    height: u16,
    colors: &Colors,
    state: &mut AppState,
    font: &FontDescription,
) -> Result<()> {
    // 背景
    cr.set_source_rgb(colors.bg.r, colors.bg.g, colors.bg.b);
    cr.paint()?;

    let pill_h = (height as f64) - 2.0 * PADDING_Y;

    // 左侧 tags
    let tags = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];
    let mut x = PADDING_X;
    for (i, label) in tags.iter().enumerate() {
        let (tw, _th) = pango_text_size(cr, font, label);
        let w = ((tw as f64) + 2.0 * PILL_HPADDING).max(40.0);

        let (bg, bw, bc, txt_color, draw_bg) = tag_visuals(colors, state.monitor_info.as_ref(), i);
        if draw_bg {
            stroke_shape_with_fill(
                cr,
                state.shape_style,
                x,
                PADDING_Y,
                w,
                pill_h,
                PILL_RADIUS,
                bw,
                bc,
                Some(bg),
            )?;
            pango_draw_text_centered(cr, font, txt_color, x, PADDING_Y, w, pill_h, label);
        }
        state.tag_rects[i] = Rect {
            x: x as i16,
            y: PADDING_Y as i16,
            w: w as u16,
            h: pill_h as u16,
        };
        x += w + TAG_SPACING;
    }

    // 布局按钮
    let layout_label = state.layout_symbol.as_str();
    let (lw, lh) = pango_text_size(cr, font, layout_label);
    let lw_total = lw as f64 + 2.0 * PILL_HPADDING;
    stroke_shape_with_fill(
        cr,
        state.shape_style,
        x,
        PADDING_Y,
        lw_total,
        pill_h,
        PILL_RADIUS,
        1.0,
        colors.green,
        Some(colors.green),
    )?;
    // 左对齐
    let ty = PADDING_Y + (pill_h - lh as f64) / 2.0 - 1.0;
    pango_draw_text_left(cr, font, colors.white, x + PILL_HPADDING, ty, layout_label);
    state.layout_button_rect = Rect {
        x: x as i16,
        y: PADDING_Y as i16,
        w: lw_total as u16,
        h: pill_h as u16,
    };
    x += lw_total + TAG_SPACING;

    // 布局选项
    if state.layout_selector_open {
        let layouts: [(&str, u32, Color); 3] = [
            ("[]=", 0, colors.green),
            ("><>", 1, colors.blue),
            ("[M]", 2, colors.purple),
        ];
        let mut opt_x = x;
        for (i, (sym, _idx, base_color)) in layouts.iter().enumerate() {
            let (tw, _th) = pango_text_size(cr, font, sym);
            let w = ((tw as f64) + 2.0 * (PILL_HPADDING - 2.0)).max(32.0);
            stroke_shape_with_fill(
                cr,
                state.shape_style,
                opt_x,
                PADDING_Y,
                w,
                pill_h,
                PILL_RADIUS,
                1.0,
                *base_color,
                Some(*base_color),
            )?;
            pango_draw_text_centered(cr, font, colors.white, opt_x, PADDING_Y, w, pill_h, sym);
            state.layout_option_rects[i] = Rect {
                x: opt_x as i16,
                y: PADDING_Y as i16,
                w: w as u16,
                h: pill_h as u16,
            };
            opt_x += w + TAG_SPACING;
        }
        // x = opt_x;
    } else {
        state.layout_option_rects = [Rect::default(), Rect::default(), Rect::default()];
    }

    // 右侧从右往左
    let mut right_x = width as f64 - PADDING_X;

    // 监视器 pill
    let mon_label = AppState::monitor_num_to_label(state.monitor_num);
    let (mon_w, mon_h) = pango_text_size(cr, font, &mon_label);
    let mon_total = mon_w as f64 + 2.0 * PILL_HPADDING;
    right_x -= mon_total + TAG_SPACING;
    stroke_shape_with_fill(
        cr,
        state.shape_style,
        right_x,
        PADDING_Y,
        mon_total,
        pill_h,
        PILL_RADIUS,
        1.0,
        colors.purple,
        Some(colors.purple),
    )?;
    let ty_mon = PADDING_Y + (pill_h - mon_h as f64) / 2.0 - 1.0;
    pango_draw_text_left(
        cr,
        font,
        colors.white,
        right_x + PILL_HPADDING,
        ty_mon,
        &mon_label,
    );

    // 时间 pill（Nerd Font 时钟：）
    let time_str = state.format_time();
    let time_label = format!(" {}", time_str);
    let (time_w, time_h) = pango_text_size(cr, font, &time_label);
    let time_total = time_w as f64 + 2.0 * PILL_HPADDING;
    right_x -= time_total + TAG_SPACING;
    stroke_shape_with_fill(
        cr,
        state.shape_style,
        right_x,
        PADDING_Y,
        time_total,
        pill_h,
        PILL_RADIUS,
        1.0,
        colors.time,
        Some(colors.time),
    )?;
    let ty_time = PADDING_Y + (pill_h - time_h as f64) / 2.0 - 1.0;
    pango_draw_text_left(
        cr,
        font,
        colors.white,
        right_x + PILL_HPADDING,
        ty_time,
        &time_label,
    );
    state.time_rect = Rect {
        x: right_x as i16,
        y: PADDING_Y as i16,
        w: time_total as u16,
        h: pill_h as u16,
    };

    // 截图 pill（Nerd Font 相机：）
    let ss_label = " Screenshot";
    let (ss_w, ss_h) = pango_text_size(cr, font, ss_label);
    let ss_total = ss_w as f64 + 2.0 * PILL_HPADDING;
    right_x -= ss_total + TAG_SPACING;
    let ss_color = if state.is_ss_hover {
        colors.orange
    } else {
        colors.teal
    };
    stroke_shape_with_fill(
        cr,
        state.shape_style,
        right_x,
        PADDING_Y,
        ss_total,
        pill_h,
        PILL_RADIUS,
        1.0,
        ss_color,
        Some(ss_color),
    )?;
    let ty_ss = PADDING_Y + (pill_h - ss_h as f64) / 2.0 - 1.0;
    pango_draw_text_left(
        cr,
        font,
        colors.white,
        right_x + PILL_HPADDING,
        ty_ss,
        ss_label,
    );
    state.ss_rect = Rect {
        x: right_x as i16,
        y: PADDING_Y as i16,
        w: ss_total as u16,
        h: pill_h as u16,
    };

    // MEM/CPU
    let (mem_total_gb, mem_used_gb, cpu_avg) =
        if let Some(snap) = state.system_monitor.get_snapshot() {
            (
                (snap.memory_total as f32) / 1e9,
                (snap.memory_used as f32) / 1e9,
                snap.cpu_average,
            )
        } else {
            (0.0, 0.0, 0.0)
        };
    let mem_usage = if mem_total_gb > 0.0 {
        (mem_used_gb / mem_total_gb) * 100.0
    } else {
        0.0
    };
    let mem_label = format!("MEM {:.0}%", mem_usage.clamp(0.0, 100.0));
    let (mem_w, mem_h) = pango_text_size(cr, font, &mem_label);
    let mem_total = mem_w as f64 + 2.0 * PILL_HPADDING;
    right_x -= mem_total + TAG_SPACING;
    let mem_bg = usage_bg_color(colors, mem_usage);
    let mem_fg = usage_text_color(colors, mem_usage);
    stroke_chamfer_with_fill(
        cr,
        right_x,
        PADDING_Y,
        mem_total,
        pill_h,
        PILL_RADIUS,
        1.0,
        mem_bg,
        Some(mem_bg),
    )?;
    let ty_mem = PADDING_Y + (pill_h - mem_h as f64) / 2.0 - 1.0;
    pango_draw_text_left(
        cr,
        font,
        mem_fg,
        right_x + PILL_HPADDING,
        ty_mem,
        &mem_label,
    );

    let cpu_label = format!("CPU {:.0}%", cpu_avg.clamp(0.0, 100.0));
    let (cpu_w, cpu_h) = pango_text_size(cr, font, &cpu_label);
    let cpu_total = cpu_w as f64 + 2.0 * PILL_HPADDING;
    right_x -= cpu_total + TAG_SPACING;
    let cpu_bg = usage_bg_color(colors, cpu_avg);
    let cpu_fg = usage_text_color(colors, cpu_avg);
    stroke_chamfer_with_fill(
        cr,
        right_x,
        PADDING_Y,
        cpu_total,
        pill_h,
        PILL_RADIUS,
        1.0,
        cpu_bg,
        Some(cpu_bg),
    )?;
    let ty_cpu = PADDING_Y + (pill_h - cpu_h as f64) / 2.0 - 1.0;
    pango_draw_text_left(
        cr,
        font,
        cpu_fg,
        right_x + PILL_HPADDING,
        ty_cpu,
        &cpu_label,
    );

    Ok(())
}

// ---------------- redraw 封装（仅在需要时调用） ----------------
use x11rb::protocol::xproto::{Gcontext, Window};
fn redraw(
    cairo_xcb: &CairoXcb,
    conn: &XCBConnection,
    back: &mut BackBuffer,
    win: Window,
    gc: Gcontext,
    width: u16,
    height: u16,
    colors: &Colors,
    state: &mut AppState,
    font: &FontDescription,
) -> Result<()> {
    let cr = back.ensure_surface(cairo_xcb)?;
    draw_bar(cr, width, height, colors, state, font)?;
    back.flush();
    back.blit_to_window(conn, win, gc)?;
    conn.flush()?;
    Ok(())
}

// ---------------- 对齐到秒的 timerfd ----------------
fn arm_second_timer(tfd: libc::c_int) -> std::io::Result<()> {
    // 对齐到下一秒：CLOCK_MONOTONIC
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let rc = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let now_ns = (ts.tv_sec as i128) * 1_000_000_000i128 + (ts.tv_nsec as i128);
    let next_sec_ns = ((ts.tv_sec as i128) + 1) * 1_000_000_000i128;
    let diff_ns = (next_sec_ns - now_ns) as i64;

    let its = libc::itimerspec {
        it_value: libc::timespec {
            tv_sec: (diff_ns / 1_000_000_000) as libc::time_t,
            tv_nsec: (diff_ns % 1_000_000_000) as libc::c_long,
        },
        it_interval: libc::timespec {
            tv_sec: 1,
            tv_nsec: 0,
        }, // 每秒触发
    };
    let rc = unsafe { libc::timerfd_settime(tfd, 0, &its as *const _, std::ptr::null_mut()) };
    if rc < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

// ---------------- eventfd 集成 shared_ring_buffer 通知 ----------------
const SHARED_TOKEN: u64 = 3;

fn spawn_shared_eventfd_notifier(
    shared_buffer: Option<std::sync::Arc<SharedRingBuffer>>,
) -> Option<libc::c_int> {
    let Some(buf) = shared_buffer.clone() else {
        return None;
    };

    // 创建 eventfd：非阻塞 + CLOEXEC
    let efd = unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) };
    if efd < 0 {
        log::error!("eventfd create failed: {}", std::io::Error::last_os_error());
        return None;
    }

    std::thread::spawn(move || {
        loop {
            match buf.wait_for_message(None) {
                Ok(true) => {
                    // 有新消息到达，通知主线程
                    let one: u64 = 1;
                    let ptr = &one as *const u64 as *const libc::c_void;
                    let r = unsafe { libc::write(efd, ptr, std::mem::size_of::<u64>()) };
                    if r < 0 {
                        let err = std::io::Error::last_os_error();
                        if let Some(code) = err.raw_os_error() {
                            // EBADF: 主线程可能已关闭 efd，退出线程
                            if code == libc::EBADF {
                                break;
                            }
                            // EAGAIN: 计数器已满（极少见），忽略；主线程稍后会读空
                            if code != libc::EAGAIN {
                                log::warn!("eventfd write error: {}", err);
                            }
                        }
                    }
                }
                Ok(false) => {}
                Err(e) => {
                    log::warn!("Shared wait failed: {}", e);
                    break;
                }
            }
        }
    });

    Some(efd)
}

// ---------------- X 事件处理（统一函数，避免重复） ----------------
fn handle_buttons(px: i16, py: i16, button: u8, state: &mut AppState) -> bool {
    let mut need_redraw = false;
    // 左侧 tag：左键 view，右键 toggle
    for (i, rect) in state.tag_rects.iter().enumerate() {
        if rect.contains(px, py) {
            if button == 1 {
                state.active_tab = i;
                state.send_tag_command_index(i, true);
            } else if button == 3 {
                state.send_tag_command_index(i, false);
            }
            need_redraw = true;
            break;
        }
    }
    // 布局按钮
    if state.layout_button_rect.contains(px, py) && button == 1 {
        state.layout_selector_open = !state.layout_selector_open;
        need_redraw = true;
    }
    // 布局选项
    for (idx, r) in state.layout_option_rects.iter().enumerate() {
        if r.w > 0 && r.contains(px, py) && button == 1 {
            state.send_layout_command(idx as u32);
            state.layout_selector_open = false;
            need_redraw = true;
            break;
        }
    }
    // 截图
    if state.ss_rect.contains(px, py) && button == 1 {
        if let Err(e) = std::process::Command::new("flameshot").arg("gui").spawn() {
            warn!("Failed to spawn flameshot: {e}");
        }
    }
    // 时间
    if state.time_rect.contains(px, py) && button == 1 {
        state.show_seconds = !state.show_seconds;
        need_redraw = true;
    }
    need_redraw
}

fn handle_x_event(
    event: x11rb::protocol::Event,
    cairo_xcb: &CairoXcb,
    conn: &XCBConnection,
    back: &mut BackBuffer,
    win: Window,
    gc: Gcontext,
    current_width: &mut u16,
    current_height: &mut u16,
    colors: &Colors,
    state: &mut AppState,
    font: &FontDescription,
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
                )?;
            }
        }
        x11rb::protocol::Event::ButtonPress(e) => {
            let px = e.event_x;
            let py = e.event_y;
            if handle_buttons(px, py, e.detail, state) {
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
    colors: &Colors,
    state: &mut AppState,
    font: &FontDescription,
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
    if let Err(e) = initialize_logging(&shared_path) {
        log::error!("Failed to initialize logging: {}", e);
        std::process::exit(1);
    }

    // 共享内存
    let shared_buffer = SharedRingBuffer::create_shared_ring_buffer_aux(&shared_path).map(Arc::new);
    // 用 eventfd 集成到 epoll
    let shared_efd = spawn_shared_eventfd_notifier(shared_buffer.clone());

    // X 连接（xcb_ffi）
    let (conn, screen_num) = XCBConnection::connect(None)?;
    let screen = &conn.setup().roots[screen_num];

    // Cairo XCB 桥接对象
    let cairo_xcb = build_cairo_xcb_by_ffi(&conn, screen)?;

    // 窗口 + GC
    let win = conn.generate_id()?;
    let gc = conn.generate_id()?;
    conn.create_gc(gc, screen.root, &CreateGCAux::new())?;

    // 创建 dock 窗口（背景 NONE，避免闪烁）
    let mut current_width = screen.width_in_pixels;
    let mut current_height = BAR_HEIGHT;
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
    use x11rb::protocol::xproto::ChangeWindowAttributesAux;
    conn.change_window_attributes(
        win,
        &ChangeWindowAttributesAux::new().background_pixmap(x11rb::NONE),
    )?;

    // 颜色
    let colors = Colors {
        bg: Color::rgb(17, 17, 17),
        text: Color::rgb(255, 255, 255),
        white: Color::rgb(255, 255, 255),
        black: Color::rgb(0, 0, 0),
        tag_colors: [
            Color::rgb(255, 107, 107), // red
            Color::rgb(78, 205, 196),  // cyan
            Color::rgb(69, 183, 209),  // blue
            Color::rgb(150, 206, 180), // green
            Color::rgb(254, 202, 87),  // yellow
            Color::rgb(255, 159, 243), // pink
            Color::rgb(84, 160, 255),  // light blue
            Color::rgb(95, 39, 205),   // purple
            Color::rgb(0, 210, 211),   // teal
        ],
        gray: Color::rgb(90, 90, 90),
        red: Color::rgb(230, 60, 60),
        green: Color::rgb(36, 179, 112),
        yellow: Color::rgb(240, 200, 40),
        orange: Color::rgb(255, 140, 0),
        blue: Color::rgb(50, 120, 220),
        purple: Color::rgb(150, 110, 210),
        teal: Color::rgb(0, 180, 180),
        time: Color::rgb(80, 150, 220),
    };

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
    )?;

    // ============ epoll + timerfd（对齐到秒） ============
    // 1) X fd
    let x_fd = conn.as_fd().as_raw_fd();

    // 2) epoll
    let epfd = unsafe { libc::epoll_create1(libc::EPOLL_CLOEXEC) };
    if epfd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }

    // 3) timerfd：对齐到秒，每秒一次
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

    // 4) 注册 epoll 事件
    const X_TOKEN: u64 = 1;
    const TFD_TOKEN: u64 = 2;

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

    // 注册 shared eventfd
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

    // 5) 主循环
    const EP_EVENTS_CAP: usize = 32;
    let mut events: [libc::epoll_event; EP_EVENTS_CAP] =
        unsafe { MaybeUninit::zeroed().assume_init() };

    // 周期性任务：时钟每秒跳动；系统监控/音频每 2 秒
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
        // 阻塞等待 epoll
        let nfds = loop {
            let n = unsafe {
                libc::epoll_wait(
                    epfd,
                    events.as_mut_ptr(),
                    EP_EVENTS_CAP as i32,
                    -1, // 永久阻塞
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
                    // 抽干所有 X 事件（内部按需 redraw）
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
                    )?;
                }
                TFD_TOKEN => {
                    // 抽干 timerfd
                    let mut buf = [0u8; 8];
                    loop {
                        let r = unsafe {
                            libc::read(tfd, buf.as_mut_ptr() as *mut libc::c_void, buf.len())
                        };
                        if r == 8 {
                            // 每次 tick：周期性任务 + 可能 redraw
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
                                )?;
                            }
                            // 继续读，抽干积压 tick
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
                                continue; // 读空所有计数
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
                                break; // 短读（不应发生）
                            }
                        }

                        let mut need_redraw = false;
                        if let Some(buf_arc) = state.shared_buffer.as_ref().cloned() {
                            // 抽干共享缓冲，保留最后一条（也可以边读边更新，但保留最后一条通常语义更合理）
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
