use anyhow::Result;
use chrono::Local;
use log::{debug, error, info, warn};
use std::env;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

// 直接复用你现有工程中的模块和类型
pub mod audio_manager;
use audio_manager::AudioManager;

pub mod error;
use error::AppError;

pub mod system_monitor;
use system_monitor::SystemMonitor;

use shared_structures::{CommandType, MonitorInfo, SharedCommand, SharedMessage, SharedRingBuffer};

// ---------------- 日志初始化----------------
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
const PADDING_X: i16 = 8;
const PADDING_Y: i16 = 4;
const PILL_RADIUS: i16 = 12;
const TAG_SPACING: i16 = 6;
const PILL_HPADDING: i16 = 10; // pill 左右内边距

fn c8(x: u8) -> u16 {
    (x as u16) << 8
} // 8bit 转 16bit（X11 颜色通道）

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

// 颜色管理
fn alloc_rgb(conn: &RustConnection, screen: &Screen, r: u8, g: u8, b: u8) -> Result<u32> {
    let c = conn
        .alloc_color(screen.default_colormap, c8(r), c8(g), c8(b))?
        .reply()?;
    Ok(c.pixel)
}
fn set_fg(conn: &RustConnection, gc: Gcontext, pixel: u32) -> Result<()> {
    conn.change_gc(gc, &ChangeGCAux::new().foreground(pixel))?;
    Ok(())
}
fn set_bg(conn: &RustConnection, gc: Gcontext, pixel: u32) -> Result<()> {
    conn.change_gc(gc, &ChangeGCAux::new().background(pixel))?;
    Ok(())
}
fn set_font(conn: &RustConnection, gc: Gcontext, font: Font) -> Result<()> {
    conn.change_gc(gc, &ChangeGCAux::new().font(font))?;
    Ok(())
}

// 圆角矩形（填充）
fn fill_round_rect(
    conn: &RustConnection,
    win: Window,
    gc: Gcontext,
    x: i16,
    y: i16,
    w: u16,
    h: u16,
    r: i16,
) -> Result<()> {
    let r = r.min(h as i16 / 2).min(w as i16 / 2).max(0);
    if r == 0 {
        conn.poly_fill_rectangle(
            win,
            gc,
            &[Rectangle {
                x,
                y,
                width: w,
                height: h,
            }],
        )?;
        return Ok(());
    }
    let w_i = w as i16;
    let h_i = h as i16;

    // 中间+两侧矩形
    let rects = &[
        Rectangle {
            x: x + r,
            y,
            width: (w_i - 2 * r) as u16,
            height: h,
        },
        Rectangle {
            x,
            y: y + r,
            width: r as u16,
            height: (h_i - 2 * r) as u16,
        },
        Rectangle {
            x: x + w_i - r,
            y: y + r,
            width: r as u16,
            height: (h_i - 2 * r) as u16,
        },
    ];
    conn.poly_fill_rectangle(win, gc, rects)?;

    // 四个角
    let ang90: i16 = 90 * 64;
    let arcs = &[
        Arc {
            x,
            y,
            width: (2 * r) as u16,
            height: (2 * r) as u16,
            angle1: ang90,
            angle2: ang90,
        },
        Arc {
            x: x + w_i - 2 * r,
            y,
            width: (2 * r) as u16,
            height: (2 * r) as u16,
            angle1: 0,
            angle2: ang90,
        },
        Arc {
            x,
            y: y + h_i - 2 * r,
            width: (2 * r) as u16,
            height: (2 * r) as u16,
            angle1: ang90 * 2,
            angle2: ang90,
        },
        Arc {
            x: x + w_i - 2 * r,
            y: y + h_i - 2 * r,
            width: (2 * r) as u16,
            height: (2 * r) as u16,
            angle1: ang90 * 3,
            angle2: ang90,
        },
    ];
    conn.poly_fill_arc(win, gc, arcs)?;
    Ok(())
}

// 画一个带“边框”的圆角矩形：通过先画边框色的外层，再画内层填充来模拟
fn stroke_round_rect(
    conn: &RustConnection,
    win: Window,
    gc: Gcontext,
    x: i16,
    y: i16,
    w: u16,
    h: u16,
    r: i16,
    border_w: i16,
    border_color: u32,
    fill_color: Option<u32>,
) -> Result<()> {
    if border_w <= 0 {
        if let Some(fill) = fill_color {
            set_fg(conn, gc, fill)?;
            fill_round_rect(conn, win, gc, x, y, w, h, r)?;
        }
        return Ok(());
    }
    // 外边框
    set_fg(conn, gc, border_color)?;
    fill_round_rect(conn, win, gc, x, y, w, h, r)?;
    // 内部填充
    if let Some(fill) = fill_color {
        let x2 = x + border_w;
        let y2 = y + border_w;
        let w2 = w.saturating_sub((border_w * 2) as u16);
        let h2 = h.saturating_sub((border_w * 2) as u16);
        if w2 > 0 && h2 > 0 {
            set_fg(conn, gc, fill)?;
            fill_round_rect(conn, win, gc, x2, y2, w2, h2, (r - border_w).max(0))?;
        }
    }
    Ok(())
}

// 文本：core font 打开、测宽、绘制
fn open_font_best_effort(conn: &RustConnection) -> Result<(Font, u16)> {
    let candidates = &["10x20", "9x15", "7x13", "fixed"];
    for name in candidates {
        let fid = conn.generate_id()?;
        if conn.open_font(fid, name.as_bytes()).is_ok() {
            let lh = if *name == "10x20" {
                20
            } else if *name == "9x15" {
                16
            } else {
                14
            };
            return Ok((fid, lh));
        }
    }
    let fid = conn.generate_id()?;
    conn.open_font(fid, b"fixed")?;
    Ok((fid, 14))
}

fn text_width(conn: &RustConnection, font: Font, s: &str) -> Result<i16> {
    // 对于 8-bit 字体，byte1=0，byte2=字符
    let chars: Vec<x11rb::protocol::xproto::Char2b> = s
        .bytes()
        .map(|b| x11rb::protocol::xproto::Char2b { byte1: 0, byte2: b })
        .collect();
    let reply = conn.query_text_extents(font, &chars)?.reply()?;
    Ok(reply.overall_width as i16)
}

fn draw_text(
    conn: &RustConnection,
    win: Window,
    gc: Gcontext,
    x: i16,
    baseline_y: i16,
    s: &str,
) -> Result<()> {
    let bytes: Vec<u8> = s.bytes().collect();
    conn.image_text8(win, gc, x, baseline_y, &bytes)?;
    Ok(())
}

// EWMH atoms
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
fn intern_atoms(conn: &RustConnection) -> Result<Atoms> {
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
    conn: &RustConnection,
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
    let top_end_x = (sw - 1) as u32;
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

// ---------------- 状态与逻辑 ----------------
#[allow(dead_code)]
struct Colors {
    bg: u32,
    text: u32,
    white: u32,
    black: u32,

    tag_colors: [u32; 9], // 工作区颜色
    gray: u32,
    red: u32,
    green: u32,
    yellow: u32,
    orange: u32,
    blue: u32,
    purple: u32,
    teal: u32,
    time: u32,
}

struct AppState {
    // 共享通讯
    shared_buffer: Option<std::sync::Arc<SharedRingBuffer>>,
    monitor_info: Option<MonitorInfo>,
    monitor_num: i32,
    layout_symbol: String,

    // 标签 UI
    tag_rects: [Rect; 9],
    active_tab: usize,

    // 布局按钮和选项
    layout_button_rect: Rect,
    layout_selector_open: bool,
    layout_option_rects: [Rect; 3],

    // 右侧 pills
    ss_rect: Rect,
    time_rect: Rect,
    is_ss_hover: bool,
    show_seconds: bool,

    // 系统与音频
    audio_manager: AudioManager,
    system_monitor: SystemMonitor,

    // 计时
    last_clock_update: Instant,
    last_monitor_update: Instant,
}

impl AppState {
    fn new(shared_buffer: Option<std::sync::Arc<SharedRingBuffer>>) -> Self {
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

    #[allow(dead_code)]
    fn send_tag_command(&mut self, is_view: bool) {
        self.send_tag_command_index(self.active_tab, is_view);
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

// usage 颜色映射
fn usage_bg_color(colors: &Colors, usage: f32) -> u32 {
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
fn usage_text_color(colors: &Colors, usage: f32) -> u32 {
    if usage <= 60.0 {
        colors.black
    } else {
        colors.white
    }
}

// tag 可视样式：返回 (bg, border_width, border_color, text_color, draw_bg)
fn tag_visuals(
    colors: &Colors,
    mi: Option<&MonitorInfo>,
    idx: usize,
) -> (u32, i16, u32, u32, bool) {
    let tag_color = colors.tag_colors[idx.min(colors.tag_colors.len() - 1)];
    if let Some(monitor) = mi {
        if let Some(status) = monitor.tag_status_vec.get(idx) {
            if status.is_urg {
                // 紧急：红底白字，边框加粗
                return (colors.red, 2, colors.red, colors.white, true);
            } else if status.is_selected {
                // 当前选中：主题色底，边框略粗
                return (tag_color, 2, tag_color, colors.black, true);
            } else if status.is_filled {
                // 有窗口（更强提示）：主题色底，但边框稍细
                return (tag_color, 1, tag_color, colors.black, true);
            } else if status.is_occ {
                // 占用但未选中：灰底白字
                return (colors.gray, 1, colors.gray, colors.white, true);
            }
        }
    }
    // 空 tag：仅描边 + 灰字，填充用 bar 背景色，始终绘制 pill
    (colors.bg, 1, colors.gray, colors.gray, true)
}

// 绘制完整 bar，返回窗口宽度（用于更新某些布局）
#[allow(unused_assignments)]
fn draw_bar(
    conn: &RustConnection,
    _screen: &Screen,
    win: Window,
    gc: Gcontext,
    font: Font,
    line_height: u16,
    colors: &Colors,
    state: &mut AppState,
    width: u16,
) -> Result<()> {
    // 背景
    set_fg(conn, gc, colors.bg)?;
    conn.poly_fill_rectangle(
        win,
        gc,
        &[Rectangle {
            x: 0,
            y: 0,
            width,
            height: BAR_HEIGHT,
        }],
    )?;

    // 文本设置
    set_font(conn, gc, font)?;

    // 基准线（粗略垂直居中）
    let pill_h = BAR_HEIGHT as i16 - PADDING_Y * 2;
    let baseline = PADDING_Y + (pill_h + line_height as i16) / 2 - 2;

    // 左侧：tags
    let tags = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];
    let mut x = PADDING_X;
    for (i, label) in tags.iter().enumerate() {
        let tw = text_width(conn, font, label)? as i16;
        let w = (tw + 2 * PILL_HPADDING).max(40);

        let (bg, bw, bc, txt_color, draw_bg) = tag_visuals(colors, state.monitor_info.as_ref(), i);

        if draw_bg {
            stroke_round_rect(
                conn,
                win,
                gc,
                x,
                PADDING_Y,
                w as u16,
                pill_h as u16,
                PILL_RADIUS,
                bw,
                bc,
                Some(bg),
            )?;
            set_fg(conn, gc, txt_color)?;
            set_bg(conn, gc, bg)?; // 关键：把文本背景设为当前 pill 底色
            let tx = x + (w - tw) / 2;
            draw_text(conn, win, gc, tx, baseline, label)?;
        }
        state.tag_rects[i] = Rect {
            x,
            y: PADDING_Y,
            w: w as u16,
            h: pill_h as u16,
        };
        x += w + TAG_SPACING;
    }

    // 布局按钮
    let layout_label = state.layout_symbol.as_str();
    let lw = text_width(conn, font, layout_label)? as i16;
    let lw_total = lw + 2 * PILL_HPADDING;
    stroke_round_rect(
        conn,
        win,
        gc,
        x,
        PADDING_Y,
        lw_total as u16,
        pill_h as u16,
        PILL_RADIUS,
        1,
        colors.green,
        Some(colors.green),
    )?;
    set_fg(conn, gc, colors.white)?;
    set_bg(conn, gc, colors.green)?;
    draw_text(conn, win, gc, x + PILL_HPADDING, baseline, layout_label)?;
    state.layout_button_rect = Rect {
        x,
        y: PADDING_Y,
        w: lw_total as u16,
        h: pill_h as u16,
    };
    x += lw_total + TAG_SPACING;

    // 如果展开布局选项：[]=/><=/[M]
    let mut opt_x = x;
    if state.layout_selector_open {
        let layouts: [(&str, u32, u32); 3] = [
            ("[]=", 0, colors.green),
            ("><>", 1, colors.blue),
            ("[M]", 2, colors.purple),
        ];
        for (i, (sym, _idx, base_color)) in layouts.iter().enumerate() {
            let tw = text_width(conn, font, sym)? as i16;
            let w = (tw + 2 * (PILL_HPADDING - 2)).max(32);
            stroke_round_rect(
                conn,
                win,
                gc,
                opt_x,
                PADDING_Y,
                w as u16,
                pill_h as u16,
                PILL_RADIUS,
                1,
                *base_color,
                Some(*base_color),
            )?;
            set_fg(conn, gc, colors.white)?;
            set_bg(conn, gc, *base_color)?;
            draw_text(conn, win, gc, opt_x + (w - tw) / 2, baseline, sym)?;
            state.layout_option_rects[i] = Rect {
                x: opt_x,
                y: PADDING_Y,
                w: w as u16,
                h: pill_h as u16,
            };
            opt_x += w + TAG_SPACING;
        }
        x = opt_x;
    } else {
        state.layout_option_rects = [Rect::default(), Rect::default(), Rect::default()];
    }

    // 右侧区域从右往左布置
    let mut right_x = width as i16 - PADDING_X;

    // 监视器 pill
    let mon_label = AppState::monitor_num_to_label(state.monitor_num);
    let mon_w = text_width(conn, font, &mon_label)? as i16 + 2 * PILL_HPADDING;
    right_x -= mon_w + TAG_SPACING;
    stroke_round_rect(
        conn,
        win,
        gc,
        right_x,
        PADDING_Y,
        mon_w as u16,
        pill_h as u16,
        PILL_RADIUS,
        1,
        colors.purple,
        Some(colors.purple),
    )?;
    set_fg(conn, gc, colors.white)?;
    set_bg(conn, gc, colors.purple)?;
    draw_text(conn, win, gc, right_x + PILL_HPADDING, baseline, &mon_label)?;

    // 时间 pill
    let time_str = state.format_time();
    let time_label = format!("TIME {}", time_str);
    let time_w = text_width(conn, font, &time_label)? as i16 + 2 * PILL_HPADDING;
    right_x -= time_w + TAG_SPACING;
    stroke_round_rect(
        conn,
        win,
        gc,
        right_x,
        PADDING_Y,
        time_w as u16,
        pill_h as u16,
        PILL_RADIUS,
        1,
        colors.time,
        Some(colors.time),
    )?;
    set_fg(conn, gc, colors.white)?;
    set_bg(conn, gc, colors.time)?;
    draw_text(
        conn,
        win,
        gc,
        right_x + PILL_HPADDING,
        baseline,
        &time_label,
    )?;
    state.time_rect = Rect {
        x: right_x,
        y: PADDING_Y,
        w: time_w as u16,
        h: pill_h as u16,
    };

    // 截图 pill（hover 变色）
    let ss_label = if state.is_ss_hover {
        "Screenshot"
    } else {
        "Screenshot"
    };
    let ss_w = text_width(conn, font, ss_label)? as i16 + 2 * PILL_HPADDING;
    right_x -= ss_w + TAG_SPACING;
    let ss_color = if state.is_ss_hover {
        colors.orange
    } else {
        colors.teal
    };
    stroke_round_rect(
        conn,
        win,
        gc,
        right_x,
        PADDING_Y,
        ss_w as u16,
        pill_h as u16,
        PILL_RADIUS,
        1,
        ss_color,
        Some(ss_color),
    )?;
    set_fg(conn, gc, colors.white)?;
    set_bg(conn, gc, ss_color)?;
    draw_text(conn, win, gc, right_x + PILL_HPADDING, baseline, ss_label)?;
    state.ss_rect = Rect {
        x: right_x,
        y: PADDING_Y,
        w: ss_w as u16,
        h: pill_h as u16,
    };

    // MEM pill
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
    let mem_w = text_width(conn, font, &mem_label)? as i16 + 2 * PILL_HPADDING;
    right_x -= mem_w + TAG_SPACING;
    let mem_bg = usage_bg_color(colors, mem_usage);
    let mem_fg = usage_text_color(colors, mem_usage);
    stroke_round_rect(
        conn,
        win,
        gc,
        right_x,
        PADDING_Y,
        mem_w as u16,
        pill_h as u16,
        PILL_RADIUS,
        1,
        mem_bg,
        Some(mem_bg),
    )?;
    set_fg(conn, gc, mem_fg)?;
    set_bg(conn, gc, mem_bg)?;
    draw_text(conn, win, gc, right_x + PILL_HPADDING, baseline, &mem_label)?;

    // CPU pill
    let cpu_label = format!("CPU {:.0}%", cpu_avg.clamp(0.0, 100.0));
    let cpu_w = text_width(conn, font, &cpu_label)? as i16 + 2 * PILL_HPADDING;
    right_x -= cpu_w + TAG_SPACING;
    let cpu_bg = usage_bg_color(colors, cpu_avg);
    let cpu_fg = usage_text_color(colors, cpu_avg);
    stroke_round_rect(
        conn,
        win,
        gc,
        right_x,
        PADDING_Y,
        cpu_w as u16,
        pill_h as u16,
        PILL_RADIUS,
        1,
        cpu_bg,
        Some(cpu_bg),
    )?;
    set_fg(conn, gc, cpu_fg)?;
    set_bg(conn, gc, cpu_bg)?;
    draw_text(conn, win, gc, right_x + PILL_HPADDING, baseline, &cpu_label)?;

    conn.flush()?;
    Ok(())
}

// ---------------- 共享内存事件线程 ----------------
enum SharedEvt {
    Updated(SharedMessage),
    Error(String),
}

fn spawn_shared_listener(
    shared_buffer: Option<std::sync::Arc<SharedRingBuffer>>,
) -> Option<mpsc::Receiver<SharedEvt>> {
    let Some(buf) = shared_buffer.clone() else {
        return None;
    };

    let (tx, rx) = mpsc::channel::<SharedEvt>();
    thread::spawn(move || {
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let stop_c = stop.clone();
        let buffer_clone = buf.clone();

        let mut prev_ts: u128 = 0;
        loop {
            match buffer_clone.wait_for_message(Some(Duration::from_secs(2))) {
                Ok(true) => match buffer_clone.try_read_latest_message() {
                    Ok(Some(msg)) => {
                        let ts = msg.timestamp as u128;
                        if ts != prev_ts {
                            prev_ts = ts;
                            if tx.send(SharedEvt::Updated(msg)).is_err() {
                                break;
                            }
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        let _ = tx.send(SharedEvt::Error(format!("Read failed: {e}")));
                        break;
                    }
                },
                Ok(false) => { /* timeout */ }
                Err(e) => {
                    let _ = tx.send(SharedEvt::Error(format!("Wait failed: {e}")));
                    break;
                }
            }
            if stop_c.load(Ordering::Relaxed) {
                break;
            }
        }
    });

    Some(rx)
}

// ---------------- 主程序 ----------------
#[allow(unused_assignments)]
fn main() -> Result<()> {
    // 参数：共用内存路径
    let args: Vec<String> = env::args().collect();
    let shared_path = args.get(1).cloned().unwrap_or_default();

    // 日志
    if let Err(e) = initialize_logging(&shared_path) {
        eprintln!("Failed to initialize logging: {}", e);
        std::process::exit(1);
    }

    // 共享内存 buffer
    let shared_buffer =
        SharedRingBuffer::create_shared_ring_buffer_aux(&shared_path).map(std::sync::Arc::new);
    let shared_rx = spawn_shared_listener(shared_buffer.clone());

    // 连接 X
    let (conn, screen_num) = x11rb::connect(None)?;
    let screen = &conn.setup().roots[screen_num];

    // 窗口与 GC
    let win = conn.generate_id()?;
    let gc = conn.generate_id()?;
    conn.create_gc(gc, screen.root, &CreateGCAux::new())?;
    let (font, line_height) = open_font_best_effort(&conn)?;

    // 创建 dock 窗口
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
            .background_pixel(screen.black_pixel)
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

    // 分配颜色
    let colors = Colors {
        bg: alloc_rgb(&conn, screen, 17, 17, 17)?, // 深灰背景
        text: alloc_rgb(&conn, screen, 255, 255, 255)?,
        white: alloc_rgb(&conn, screen, 255, 255, 255)?,
        black: alloc_rgb(&conn, screen, 0, 0, 0)?,

        tag_colors: [
            alloc_rgb(&conn, screen, 255, 107, 107)?, // red
            alloc_rgb(&conn, screen, 78, 205, 196)?,  // cyan
            alloc_rgb(&conn, screen, 69, 183, 209)?,  // blue
            alloc_rgb(&conn, screen, 150, 206, 180)?, // green
            alloc_rgb(&conn, screen, 254, 202, 87)?,  // yellow
            alloc_rgb(&conn, screen, 255, 159, 243)?, // pink
            alloc_rgb(&conn, screen, 84, 160, 255)?,  // light blue
            alloc_rgb(&conn, screen, 95, 39, 205)?,   // purple
            alloc_rgb(&conn, screen, 0, 210, 211)?,   // teal
        ],
        gray: alloc_rgb(&conn, screen, 90, 90, 90)?,
        red: alloc_rgb(&conn, screen, 230, 60, 60)?,
        green: alloc_rgb(&conn, screen, 36, 179, 112)?,
        yellow: alloc_rgb(&conn, screen, 240, 200, 40)?,
        orange: alloc_rgb(&conn, screen, 255, 140, 0)?,
        blue: alloc_rgb(&conn, screen, 50, 120, 220)?,
        purple: alloc_rgb(&conn, screen, 150, 110, 210)?,
        teal: alloc_rgb(&conn, screen, 0, 180, 180)?,
        time: alloc_rgb(&conn, screen, 80, 150, 220)?,
    };
    use x11rb::protocol::xproto::ChangeWindowAttributesAux;
    conn.change_window_attributes(
        win,
        &ChangeWindowAttributesAux::new().background_pixel(colors.bg),
    )?;

    // 初始状态
    let mut state = AppState::new(shared_buffer);

    // 初次绘制
    draw_bar(
        &conn,
        screen,
        win,
        gc,
        font,
        line_height,
        &colors,
        &mut state,
        current_width,
    )?;

    loop {
        // 处理 X 事件
        while let Some(event) = conn.poll_for_event()? {
            match event {
                Event::Expose(_e) => {
                    if let Ok(geo) = conn.get_geometry(win)?.reply() {
                        current_width = geo.width;
                        current_height = geo.height;
                    }
                    draw_bar(
                        &conn,
                        screen,
                        win,
                        gc,
                        font,
                        line_height,
                        &colors,
                        &mut state,
                        current_width,
                    )?;
                }
                Event::ConfigureNotify(e) => {
                    if e.window == win {
                        current_width = e.width as u16;
                        current_height = e.height as u16;
                        draw_bar(
                            &conn,
                            screen,
                            win,
                            gc,
                            font,
                            line_height,
                            &colors,
                            &mut state,
                            current_width,
                        )?;
                    }
                }
                Event::MotionNotify(e) => {
                    let hovered = state.ss_rect.contains(e.event_x, e.event_y);
                    if hovered != state.is_ss_hover {
                        state.is_ss_hover = hovered;
                        draw_bar(
                            &conn,
                            screen,
                            win,
                            gc,
                            font,
                            line_height,
                            &colors,
                            &mut state,
                            current_width,
                        )?;
                    }
                }
                Event::ButtonPress(e) => {
                    let px = e.event_x;
                    let py = e.event_y;
                    // 左侧 tag：左键 view，右键 toggle
                    for (i, rect) in state.tag_rects.iter().enumerate() {
                        if rect.contains(px, py) {
                            if e.detail == 1 {
                                state.active_tab = i;
                                state.send_tag_command_index(i, true);
                            } else if e.detail == 3 {
                                state.send_tag_command_index(i, false);
                            }
                            draw_bar(
                                &conn,
                                screen,
                                win,
                                gc,
                                font,
                                line_height,
                                &colors,
                                &mut state,
                                current_width,
                            )?;
                            break;
                        }
                    }
                    // 布局按钮
                    if state.layout_button_rect.contains(px, py) && e.detail == 1 {
                        state.layout_selector_open = !state.layout_selector_open;
                        draw_bar(
                            &conn,
                            screen,
                            win,
                            gc,
                            font,
                            line_height,
                            &colors,
                            &mut state,
                            current_width,
                        )?;
                    }
                    // 布局选项
                    for (idx, r) in state.layout_option_rects.iter().enumerate() {
                        if r.w > 0 && r.contains(px, py) && e.detail == 1 {
                            state.send_layout_command(idx as u32);
                            state.layout_selector_open = false;
                            draw_bar(
                                &conn,
                                screen,
                                win,
                                gc,
                                font,
                                line_height,
                                &colors,
                                &mut state,
                                current_width,
                            )?;
                            break;
                        }
                    }
                    // 截图
                    if state.ss_rect.contains(px, py) && e.detail == 1 {
                        if let Err(e) = Command::new("flameshot").arg("gui").spawn() {
                            warn!("Failed to spawn flameshot: {e}");
                        }
                    }
                    // 时间
                    if state.time_rect.contains(px, py) && e.detail == 1 {
                        state.show_seconds = !state.show_seconds;
                        draw_bar(
                            &conn,
                            screen,
                            win,
                            gc,
                            font,
                            line_height,
                            &colors,
                            &mut state,
                            current_width,
                        )?;
                    }
                }
                _ => {}
            }
        }

        // 处理共享内存更新
        if let Some(rx) = &shared_rx {
            loop {
                match rx.try_recv() {
                    Ok(SharedEvt::Updated(msg)) => {
                        state.update_from_shared(msg);
                        // 立即重绘
                        draw_bar(
                            &conn,
                            screen,
                            win,
                            gc,
                            font,
                            line_height,
                            &colors,
                            &mut state,
                            current_width,
                        )?;
                    }
                    Ok(SharedEvt::Error(err_msg)) => {
                        warn!("SharedMemoryError: {err_msg}");
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => break,
                }
            }
        }

        // 定时刷新（时间每秒，系统信息/音频每 2 秒）
        if state.last_clock_update.elapsed() >= Duration::from_millis(1000) {
            state.last_clock_update = Instant::now();

            // 节流：系统监控/音频
            if state.last_monitor_update.elapsed() >= Duration::from_secs(2) {
                state.system_monitor.update_if_needed();
                state.audio_manager.update_if_needed();
                state.last_monitor_update = Instant::now();
            }

            // 每秒重绘（时间）
            draw_bar(
                &conn,
                screen,
                win,
                gc,
                font,
                line_height,
                &colors,
                &mut state,
                current_width,
            )?;
        }

        thread::sleep(Duration::from_millis(10));
    }
}
