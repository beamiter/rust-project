// bar_core/src/lib.rs
// 单文件版核心库：UI 颜色/形状/Pango 文本、AppState、绘制、timerfd、eventfd、日志
// 依赖：anyhow, cairo-rs(xcb), pango, pangocairo, flexi_logger, log, libc, chrono, shared_structures

use anyhow::Result;
use cairo::Context;
use chrono::Local;
use libc;
use log::{debug, error, info, warn};
use pango::FontDescription;
use pangocairo::functions::{create_layout, show_layout};
use shared_structures::{CommandType, MonitorInfo, SharedCommand, SharedMessage, SharedRingBuffer};
use std::sync::Arc;
use std::time::Instant;

use std::f64::consts::{FRAC_PI_2, PI};

pub mod audio_manager;
pub mod system_monitor;
pub use audio_manager::AudioManager;
pub use system_monitor::SystemMonitor;

// ================= 公共类型 =================

#[derive(Clone, Copy, Debug)]
pub struct Color {
    pub r: f64,
    pub g: f64,
    pub b: f64,
}
impl Color {
    pub fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self {
            r: r as f64 / 255.0,
            g: g as f64 / 255.0,
            b: b as f64 / 255.0,
        }
    }

    // 轻量化 hover 需要的辅助方法：提亮 / 变暗
    pub fn lighten(&self, amount: f64) -> Self {
        let a = amount.clamp(0.0, 1.0);
        Self {
            r: (self.r + (1.0 - self.r) * a).clamp(0.0, 1.0),
            g: (self.g + (1.0 - self.g) * a).clamp(0.0, 1.0),
            b: (self.b + (1.0 - self.b) * a).clamp(0.0, 1.0),
        }
    }
    pub fn darken(&self, amount: f64) -> Self {
        let a = amount.clamp(0.0, 1.0);
        Self {
            r: (self.r * (1.0 - a)).clamp(0.0, 1.0),
            g: (self.g * (1.0 - a)).clamp(0.0, 1.0),
            b: (self.b * (1.0 - a)).clamp(0.0, 1.0),
        }
    }
}

#[derive(Clone)]
pub struct Colors {
    pub bg: Color,
    pub text: Color,
    pub white: Color,
    pub black: Color,
    pub tag_colors: [Color; 9],
    pub gray: Color,
    pub red: Color,
    pub green: Color,
    pub yellow: Color,
    pub orange: Color,
    pub blue: Color,
    pub purple: Color,
    pub teal: Color,
    pub time: Color,
}

pub fn default_colors() -> Colors {
    Colors {
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
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Rect {
    pub x: i16,
    pub y: i16,
    pub w: u16,
    pub h: u16,
}
impl Rect {
    pub fn contains(&self, px: i16, py: i16) -> bool {
        px >= self.x
            && py >= self.y
            && (px as i32) < (self.x as i32 + self.w as i32)
            && (py as i32) < (self.y as i32 + self.h as i32)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum ShapeStyle {
    Chamfer,
    Pill,
}

#[derive(Clone, Debug)]
pub struct BarConfig {
    pub bar_height: u16,
    pub padding_x: f64,
    pub padding_y: f64,
    pub tag_spacing: f64,
    pub pill_hpadding: f64,
    pub pill_radius: f64,
    pub shape_style: ShapeStyle,
    pub time_icon: &'static str,
    pub screenshot_label: &'static str,
}
impl Default for BarConfig {
    fn default() -> Self {
        Self {
            bar_height: 40,
            padding_x: 8.0,
            padding_y: 4.0,
            tag_spacing: 6.0,
            pill_hpadding: 10.0,
            pill_radius: 6.0,
            shape_style: ShapeStyle::Pill,
            time_icon: "",
            screenshot_label: " Screenshot",
        }
    }
}

// ================= AppState 与业务逻辑 =================

pub struct AppState {
    pub shared_buffer: Option<Arc<SharedRingBuffer>>,
    pub monitor_info: Option<MonitorInfo>,
    pub monitor_num: i32,
    pub layout_symbol: String,

    pub tag_rects: [Rect; 9],
    pub active_tab: usize,

    pub layout_button_rect: Rect,
    pub layout_selector_open: bool,
    pub layout_option_rects: [Rect; 3],

    pub ss_rect: Rect,
    pub time_rect: Rect,
    pub is_ss_hover: bool,
    pub show_seconds: bool,

    // Hover 状态
    pub hover_target: HoverTarget,

    // hover 判定区域
    pub mem_rect: Rect,
    pub cpu_rect: Rect,
    pub mon_rect: Rect,

    pub audio_manager: AudioManager,
    pub system_monitor: SystemMonitor,

    pub last_clock_update: Instant,
    pub last_monitor_update: Instant,

    pub shape_style: ShapeStyle,
}

// 排他式 hover 的命中目标
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoverTarget {
    None,
    Tag(usize),
    LayoutOption(usize),
    LayoutButton,
    Screenshot,
    Time,
    Mem,
    Cpu,
    Monitor,
}

impl AppState {
    pub fn new(shared_buffer: Option<Arc<SharedRingBuffer>>) -> Self {
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

            hover_target: HoverTarget::None,

            mem_rect: Rect::default(),
            cpu_rect: Rect::default(),
            mon_rect: Rect::default(),

            audio_manager: AudioManager::new(),
            system_monitor: SystemMonitor::new(5),

            last_clock_update: Instant::now(),
            last_monitor_update: Instant::now(),

            shape_style: ShapeStyle::Pill,
        }
    }
    pub fn monitor_num_to_label(num: i32) -> String {
        format!("M{}", num)
    }
    pub fn update_from_shared(&mut self, msg: SharedMessage) {
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
    pub fn send_tag_command_index(&mut self, idx: usize, is_view: bool) {
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
    pub fn send_layout_command(&mut self, layout_index: u32) {
        let cmd = SharedCommand::new(CommandType::SetLayout, layout_index, self.monitor_num);
        if let Some(buf) = &self.shared_buffer {
            match buf.send_command(cmd) {
                Ok(true) => info!("Sent command: {:?} by shared_buffer", cmd),
                Ok(false) => warn!("Command buffer full, command dropped"),
                Err(e) => error!("Failed to send command: {}", e),
            }
        }
    }
    pub fn format_time(&self) -> String {
        let now = Local::now();
        if self.show_seconds {
            now.format("%Y-%m-%d %H:%M:%S").to_string()
        } else {
            now.format("%Y-%m-%d %H:%M").to_string()
        }
    }
    pub fn handle_buttons(&mut self, px: i16, py: i16, button: u8) -> bool {
        let mut need_redraw = false;
        // 左侧 tag：左键 view，右键 toggle
        for (i, rect) in self.tag_rects.iter().enumerate() {
            if rect.contains(px, py) {
                if button == 1 {
                    self.active_tab = i;
                    self.send_tag_command_index(i, true);
                } else if button == 3 {
                    self.send_tag_command_index(i, false);
                }
                need_redraw = true;
                break;
            }
        }
        // 布局按钮
        if self.layout_button_rect.contains(px, py) && button == 1 {
            self.layout_selector_open = !self.layout_selector_open;
            need_redraw = true;
        }
        // 布局选项
        for (idx, r) in self.layout_option_rects.iter().enumerate() {
            if r.w > 0 && r.contains(px, py) && button == 1 {
                self.send_layout_command(idx as u32);
                self.layout_selector_open = false;
                need_redraw = true;
                break;
            }
        }
        // 截图
        if self.ss_rect.contains(px, py) && button == 1 {
            if let Err(e) = std::process::Command::new("flameshot").arg("gui").spawn() {
                warn!("Failed to spawn flameshot: {e}");
            }
        }
        // 时间 pill 切换秒显示
        if self.time_rect.contains(px, py) && button == 1 {
            self.show_seconds = !self.show_seconds;
            need_redraw = true;
        }
        need_redraw
    }

    // 命中测试：排他式，按优先级选1个
    fn hit_test(&self, px: i16, py: i16) -> HoverTarget {
        // 1) 布局选项（仅在打开时）
        if self.layout_selector_open {
            for (i, r) in self.layout_option_rects.iter().enumerate() {
                if r.w > 0 && r.contains(px, py) {
                    return HoverTarget::LayoutOption(i);
                }
            }
        }
        // 2) 布局按钮
        if self.layout_button_rect.contains(px, py) {
            return HoverTarget::LayoutButton;
        }
        // 3) 右侧 pills（按你喜欢的优先级；这里时间优先）
        if self.time_rect.contains(px, py) {
            return HoverTarget::Time;
        }
        if self.ss_rect.contains(px, py) {
            return HoverTarget::Screenshot;
        }
        if self.mem_rect.contains(px, py) {
            return HoverTarget::Mem;
        }
        if self.cpu_rect.contains(px, py) {
            return HoverTarget::Cpu;
        }
        if self.mon_rect.contains(px, py) {
            return HoverTarget::Monitor;
        }
        // 4) 左侧 tags（只取第一个命中的）
        for (i, rect) in self.tag_rects.iter().enumerate() {
            if rect.contains(px, py) {
                return HoverTarget::Tag(i);
            }
        }
        HoverTarget::None
    }

    // 鼠标移动：更新 hover 状态。返回是否需要重绘（排他式）
    pub fn update_hover(&mut self, px: i16, py: i16) -> bool {
        self.hover_target = self.hit_test(px, py);
        return self.hover_target != HoverTarget::None;
    }

    // 鼠标离开：清空 hover 状态。返回是否需要重绘
    pub fn clear_hover(&mut self) -> bool {
        let changed = self.hover_target != HoverTarget::None;
        self.hover_target = HoverTarget::None;
        changed
    }
}

// ================= 绘制相关：Pango 文字与形状 =================

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

// 使用率配色
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

// ================= 对外：绘制入口 =================

pub fn draw_bar(
    cr: &Context,
    width: u16,
    height: u16,
    colors: &Colors,
    state: &mut AppState,
    font: &FontDescription,
    cfg: &BarConfig,
) -> Result<()> {
    // 背景
    cr.set_source_rgb(colors.bg.r, colors.bg.g, colors.bg.b);
    cr.paint()?;

    let pill_h = (height as f64) - 2.0 * cfg.padding_y;

    // 左侧 tags
    let tags = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];
    let mut x = cfg.padding_x;
    for (i, label) in tags.iter().enumerate() {
        let (tw, _th) = pango_text_size(cr, font, label);
        let w = ((tw as f64) + 2.0 * cfg.pill_hpadding).max(40.0);

        let (mut bg, mut bw, mut bc, txt_color, draw_bg) =
            tag_visuals(colors, state.monitor_info.as_ref(), i);

        // Hover 样式：提亮 + 边框加粗
        if HoverTarget::Tag(i) == state.hover_target {
            bg = bg.lighten(0.10);
            bc = bc.lighten(0.12);
            bw = (bw + 1.0).min(3.0);
        }

        if draw_bg {
            stroke_shape_with_fill(
                cr,
                state.shape_style,
                x,
                cfg.padding_y,
                w,
                pill_h,
                cfg.pill_radius,
                bw,
                bc,
                Some(bg),
            )?;
            pango_draw_text_centered(cr, font, txt_color, x, cfg.padding_y, w, pill_h, label);
        }
        state.tag_rects[i] = Rect {
            x: x as i16,
            y: cfg.padding_y as i16,
            w: w as u16,
            h: pill_h as u16,
        };
        x += w + cfg.tag_spacing;
    }

    // 布局按钮
    let layout_label = state.layout_symbol.as_str();
    let (lw, lh) = pango_text_size(cr, font, layout_label);
    let lw_total = lw as f64 + 2.0 * cfg.pill_hpadding;

    let mut layout_fill = colors.green;
    let mut layout_border = colors.green;
    let mut layout_bw = 1.0;
    if state.hover_target == HoverTarget::LayoutButton {
        layout_fill = layout_fill.lighten(0.08);
        layout_border = layout_border.lighten(0.12);
        layout_bw = 2.0;
    }
    stroke_shape_with_fill(
        cr,
        state.shape_style,
        x,
        cfg.padding_y,
        lw_total,
        pill_h,
        cfg.pill_radius,
        layout_bw,
        layout_border,
        Some(layout_fill),
    )?;
    let ty = cfg.padding_y + (pill_h - lh as f64) / 2.0 - 1.0;
    pango_draw_text_left(
        cr,
        font,
        colors.white,
        x + cfg.pill_hpadding,
        ty,
        layout_label,
    );
    state.layout_button_rect = Rect {
        x: x as i16,
        y: cfg.padding_y as i16,
        w: lw_total as u16,
        h: pill_h as u16,
    };
    x += lw_total + cfg.tag_spacing;

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
            let w = ((tw as f64) + 2.0 * (cfg.pill_hpadding - 2.0)).max(32.0);

            let mut fill = *base_color;
            let mut border = *base_color;
            let mut bw = 1.0;
            if HoverTarget::LayoutOption(i) == state.hover_target {
                fill = fill.lighten(0.08);
                border = border.lighten(0.12);
                bw = 2.0;
            }
            stroke_shape_with_fill(
                cr,
                state.shape_style,
                opt_x,
                cfg.padding_y,
                w,
                pill_h,
                cfg.pill_radius,
                bw,
                border,
                Some(fill),
            )?;
            pango_draw_text_centered(cr, font, colors.white, opt_x, cfg.padding_y, w, pill_h, sym);
            state.layout_option_rects[i] = Rect {
                x: opt_x as i16,
                y: cfg.padding_y as i16,
                w: w as u16,
                h: pill_h as u16,
            };
            opt_x += w + cfg.tag_spacing;
        }
    } else {
        state.layout_option_rects = [Rect::default(), Rect::default(), Rect::default()];
    }

    // 右侧从右往左
    let mut right_x = width as f64 - cfg.padding_x;

    // 监视器 pill
    let mon_label = AppState::monitor_num_to_label(state.monitor_num);
    let (mon_w, mon_h) = pango_text_size(cr, font, &mon_label);
    let mon_total = mon_w as f64 + 2.0 * cfg.pill_hpadding;
    right_x -= mon_total + cfg.tag_spacing;
    let mut mon_fill = colors.purple;
    let mut mon_border = colors.purple;
    let mut mon_bw = 1.0;
    if HoverTarget::Monitor == state.hover_target {
        mon_fill = mon_fill.lighten(0.08);
        mon_border = mon_border.lighten(0.12);
        mon_bw = 2.0;
    }
    stroke_shape_with_fill(
        cr,
        state.shape_style,
        right_x,
        cfg.padding_y,
        mon_total,
        pill_h,
        cfg.pill_radius,
        mon_bw,
        mon_border,
        Some(mon_fill),
    )?;
    let ty_mon = cfg.padding_y + (pill_h - mon_h as f64) / 2.0 - 1.0;
    pango_draw_text_left(
        cr,
        font,
        colors.white,
        right_x + cfg.pill_hpadding,
        ty_mon,
        &mon_label,
    );
    state.mon_rect = Rect {
        x: right_x as i16,
        y: cfg.padding_y as i16,
        w: mon_total as u16,
        h: pill_h as u16,
    };

    // 时间 pill
    let time_str = state.format_time();
    let time_label = format!("{} {}", cfg.time_icon, time_str);
    let (time_w, time_h) = pango_text_size(cr, font, &time_label);
    let time_total = time_w as f64 + 2.0 * cfg.pill_hpadding;
    right_x -= time_total + cfg.tag_spacing;
    let mut time_fill = colors.time;
    let mut time_border = colors.time;
    let mut time_bw = 1.0;
    if HoverTarget::Time == state.hover_target {
        time_fill = time_fill.lighten(0.08);
        time_border = time_border.lighten(0.12);
        time_bw = 2.0;
    }
    stroke_shape_with_fill(
        cr,
        state.shape_style,
        right_x,
        cfg.padding_y,
        time_total,
        pill_h,
        cfg.pill_radius,
        time_bw,
        time_border,
        Some(time_fill),
    )?;
    let ty_time = cfg.padding_y + (pill_h - time_h as f64) / 2.0 - 1.0;
    pango_draw_text_left(
        cr,
        font,
        colors.white,
        right_x + cfg.pill_hpadding,
        ty_time,
        &time_label,
    );
    state.time_rect = Rect {
        x: right_x as i16,
        y: cfg.padding_y as i16,
        w: time_total as u16,
        h: pill_h as u16,
    };

    // 截图 pill（hover：提亮 + 边框加粗）
    let ss_label = cfg.screenshot_label;
    let (ss_w, ss_h) = pango_text_size(cr, font, ss_label);
    let ss_total = ss_w as f64 + 2.0 * cfg.pill_hpadding;
    right_x -= ss_total + cfg.tag_spacing;
    let mut ss_fill = colors.teal;
    let mut ss_border = colors.teal;
    let mut ss_bw = 1.0;
    if state.is_ss_hover {
        ss_fill = ss_fill.lighten(0.08);
        ss_border = ss_border.lighten(0.12);
        ss_bw = 2.0;
    }
    stroke_shape_with_fill(
        cr,
        state.shape_style,
        right_x,
        cfg.padding_y,
        ss_total,
        pill_h,
        cfg.pill_radius,
        ss_bw,
        ss_border,
        Some(ss_fill),
    )?;
    let ty_ss = cfg.padding_y + (pill_h - ss_h as f64) / 2.0 - 1.0;
    pango_draw_text_left(
        cr,
        font,
        colors.white,
        right_x + cfg.pill_hpadding,
        ty_ss,
        ss_label,
    );
    state.ss_rect = Rect {
        x: right_x as i16,
        y: cfg.padding_y as i16,
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
    let mem_total = mem_w as f64 + 2.0 * cfg.pill_hpadding;
    right_x -= mem_total + cfg.tag_spacing;
    let base_mem_bg = usage_bg_color(colors, mem_usage);
    let base_mem_fg = usage_text_color(colors, mem_usage);
    let mut mem_bg = base_mem_bg;
    let mut mem_border = base_mem_bg;
    let mut mem_bw = 1.0;
    if HoverTarget::Mem == state.hover_target {
        mem_bg = mem_bg.lighten(0.08);
        mem_border = mem_border.lighten(0.12);
        mem_bw = 2.0;
    }
    stroke_shape_with_fill(
        cr,
        state.shape_style,
        right_x,
        cfg.padding_y,
        mem_total,
        pill_h,
        cfg.pill_radius,
        mem_bw,
        mem_border,
        Some(mem_bg),
    )?;
    let ty_mem = cfg.padding_y + (pill_h - mem_h as f64) / 2.0 - 1.0;
    pango_draw_text_left(
        cr,
        font,
        base_mem_fg,
        right_x + cfg.pill_hpadding,
        ty_mem,
        &mem_label,
    );
    state.mem_rect = Rect {
        x: right_x as i16,
        y: cfg.padding_y as i16,
        w: mem_total as u16,
        h: pill_h as u16,
    };

    let cpu_label = format!("CPU {:.0}%", cpu_avg.clamp(0.0, 100.0));
    let (cpu_w, cpu_h) = pango_text_size(cr, font, &cpu_label);
    let cpu_total = cpu_w as f64 + 2.0 * cfg.pill_hpadding;
    right_x -= cpu_total + cfg.tag_spacing;
    let base_cpu_bg = usage_bg_color(colors, cpu_avg);
    let base_cpu_fg = usage_text_color(colors, cpu_avg);
    let mut cpu_bg = base_cpu_bg;
    let mut cpu_border = base_cpu_bg;
    let mut cpu_bw = 1.0;
    if HoverTarget::Cpu == state.hover_target {
        cpu_bg = cpu_bg.lighten(0.08);
        cpu_border = cpu_border.lighten(0.12);
        cpu_bw = 2.0;
    }
    stroke_shape_with_fill(
        cr,
        state.shape_style,
        right_x,
        cfg.padding_y,
        cpu_total,
        pill_h,
        cfg.pill_radius,
        cpu_bw,
        cpu_border,
        Some(cpu_bg),
    )?;
    let ty_cpu = cfg.padding_y + (pill_h - cpu_h as f64) / 2.0 - 1.0;
    pango_draw_text_left(
        cr,
        font,
        base_cpu_fg,
        right_x + cfg.pill_hpadding,
        ty_cpu,
        &cpu_label,
    );
    state.cpu_rect = Rect {
        x: right_x as i16,
        y: cfg.padding_y as i16,
        w: cpu_total as u16,
        h: pill_h as u16,
    };

    Ok(())
}

// ================= timerfd 对齐到秒 =================

pub fn arm_second_timer(tfd: libc::c_int) -> std::io::Result<()> {
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
        },
    };
    let rc = unsafe { libc::timerfd_settime(tfd, 0, &its as *const _, std::ptr::null_mut()) };
    if rc < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

// ================= eventfd 集成 shared_ring_buffer 通知 =================

pub const SHARED_TOKEN: u64 = 3;

pub fn spawn_shared_eventfd_notifier(
    shared_buffer: Option<Arc<SharedRingBuffer>>,
    non_block: bool,
) -> Option<libc::c_int> {
    let Some(buf) = shared_buffer.clone() else {
        return None;
    };

    // 创建 eventfd：非阻塞 + CLOEXEC
    let efd = unsafe {
        if non_block {
            libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC)
        } else {
            libc::eventfd(0, libc::EFD_CLOEXEC)
        }
    };
    if efd < 0 {
        error!("eventfd create failed: {}", std::io::Error::last_os_error());
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
                            // EAGAIN: 计数器已满（极少见），忽略
                            if code != libc::EAGAIN {
                                warn!("eventfd write error: {}", err);
                            }
                        }
                    }
                }
                Ok(false) => {}
                Err(e) => {
                    warn!("Shared wait failed: {}", e);
                    break;
                }
            }
        }
    });

    Some(efd)
}

// ================= 日志初始化 =================

pub fn initialize_logging(program_name: &str, shared_path: &str) -> Result<()> {
    use chrono::Local as ChronoLocal;
    use flexi_logger::{Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming};

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
        program_name.to_string()
    } else {
        std::path::Path::new(shared_path)
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| format!("{}_{}", program_name, name))
            .unwrap_or_else(|| program_name.to_string())
    };

    let log_filename = format!("{}_{}", file_name, timestamp);
    let log_spec = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());

    Logger::try_with_str(log_spec)?
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
        .start()?;

    info!("Log directory: {}", log_dir);
    Ok(())
}
