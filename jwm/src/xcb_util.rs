// src/xcb_util.rs

use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;

x11rb::atom_manager! {
    pub Atoms: AtomsCookie {
        WM_PROTOCOLS,
        WM_DELETE_WINDOW,
        WM_STATE,
        WM_TAKE_FOCUS,
        WM_TRANSIENT_FOR,

        _NET_ACTIVE_WINDOW,
        _NET_SUPPORTED,
        _NET_WM_NAME,
        _NET_WM_PID,
        _NET_WM_STATE,
        _NET_SUPPORTING_WM_CHECK,
        _NET_WM_STATE_FULLSCREEN,
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_DIALOG,
        _NET_CLIENT_LIST,
        _NET_CLIENT_LIST_STACKING,
        _NET_CLIENT_INFO,
        _NET_WM_STRUT,
        _NET_WM_STRUT_PARTIAL,
        _NET_WM_WINDOW_TYPE_POPUP_MENU,
        _NET_WM_WINDOW_TYPE_DROPDOWN_MENU,
        _NET_WM_WINDOW_TYPE_MENU,
        _NET_WM_WINDOW_TYPE_TOOLTIP,
        _NET_WM_WINDOW_TYPE_COMBO,
        _NET_WM_WINDOW_TYPE_NOTIFICATION,

        UTF8_STRING,
        COMPOUND_TEXT,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StandardCursor {
    XCursor = 0,
    Arrow = 2,
    BasedArrowDown = 4,
    BasedArrowUp = 6,
    Boat = 8,
    Bogosity = 10,
    BottomLeftCorner = 12,
    BottomRightCorner = 14,
    BottomSide = 16,
    BottomTee = 18,
    BoxSpiral = 20,
    CenterPtr = 22,
    Circle = 24,
    Clock = 26,
    CoffeeMug = 28,
    Cross = 30,
    CrossReverse = 32,
    Crosshair = 34,
    DiamondCross = 36,
    Dot = 38,
    Dotbox = 40,
    DoubleArrow = 42,
    DraftLarge = 44,
    DraftSmall = 46,
    DrapedBox = 48,
    Exchange = 50,
    Fleur = 52,
    Gobbler = 54,
    Gumby = 56,
    Hand1 = 58,
    Hand2 = 60,
    Heart = 62,
    Icon = 64,
    IronCross = 66,
    LeftPtr = 68,
    LeftSide = 70,
    LeftTee = 72,
    Leftbutton = 74,
    LlAngle = 76,
    LrAngle = 78,
    Man = 80,
    Middlebutton = 82,
    Mouse = 84,
    Pencil = 86,
    Pirate = 88,
    Plus = 90,
    QuestionArrow = 92,
    RightPtr = 94,
    RightSide = 96,
    RightTee = 98,
    Rightbutton = 100,
    RtlLogo = 102,
    Sailboat = 104,
    SbDownArrow = 106,
    SbHDoubleArrow = 108,
    SbLeftArrow = 110,
    SbRightArrow = 112,
    SbUpArrow = 114,
    SbVDoubleArrow = 116,
    Shuttle = 118,
    Sizing = 120,
    Spider = 122,
    Spraycan = 124,
    Star = 126,
    Target = 128,
    Tcross = 130,
    TopLeftArrow = 132,
    TopLeftCorner = 134,
    TopRightCorner = 136,
    TopSide = 138,
    TopTee = 140,
    Trek = 142,
    UlAngle = 144,
    Umbrella = 146,
    UrAngle = 148,
    Watch = 150,
    Xterm = 152,
}

impl StandardCursor {
    /// 创建光标
    pub fn create(
        &self,
        conn: &impl Connection,
        font: Font,
    ) -> Result<Cursor, Box<dyn std::error::Error>> {
        let cursor_id = conn.generate_id()?;
        let glyph = *self as u16;
        conn.create_glyph_cursor(
            cursor_id,
            font,
            font,
            glyph,
            glyph + 1,
            0,
            0,
            0, // 黑色前景
            65535,
            65535,
            65535, // 白色背景
        )?;
        Ok(cursor_id)
    }

    /// 创建自定义颜色的光标
    pub fn create_colored(
        &self,
        conn: &impl Connection,
        font: Font,
        fg_r: u16,
        fg_g: u16,
        fg_b: u16,
        bg_r: u16,
        bg_g: u16,
        bg_b: u16,
    ) -> Result<Cursor, Box<dyn std::error::Error>> {
        let cursor_id = conn.generate_id()?;
        let glyph = *self as u16;
        conn.create_glyph_cursor(
            cursor_id,
            font,
            font,
            glyph,
            glyph + 1,
            fg_r,
            fg_g,
            fg_b,
            bg_r,
            bg_g,
            bg_b,
        )?;
        Ok(cursor_id)
    }

    /// 获取光标的描述
    pub fn description(&self) -> &'static str {
        match self {
            Self::XCursor => "Default X cursor",
            Self::Arrow => "Standard arrow",
            Self::BasedArrowDown => "Down arrow",
            Self::BasedArrowUp => "Up arrow",
            Self::Boat => "Boat shape",
            Self::Bogosity => "Error/invalid indicator",
            Self::BottomLeftCorner => "Bottom-left corner resize",
            Self::BottomRightCorner => "Bottom-right corner resize",
            Self::BottomSide => "Bottom side resize",
            Self::BottomTee => "Bottom T shape",
            Self::BoxSpiral => "Box spiral",
            Self::CenterPtr => "Center pointer",
            Self::Circle => "Circle",
            Self::Clock => "Clock/waiting",
            Self::CoffeeMug => "Coffee mug",
            Self::Cross => "Cross",
            Self::CrossReverse => "Reverse cross",
            Self::Crosshair => "Crosshair",
            Self::DiamondCross => "Diamond cross",
            Self::Dot => "Dot",
            Self::Dotbox => "Dotted box",
            Self::DoubleArrow => "Double arrow",
            Self::DraftLarge => "Large draft",
            Self::DraftSmall => "Small draft",
            Self::DrapedBox => "Draped box",
            Self::Exchange => "Exchange",
            Self::Fleur => "Four-way move",
            Self::Gobbler => "Pac-man",
            Self::Gumby => "Gumby character",
            Self::Hand1 => "Hand pointer 1",
            Self::Hand2 => "Hand pointer 2",
            Self::Heart => "Heart shape",
            Self::Icon => "Icon",
            Self::IronCross => "Iron cross",
            Self::LeftPtr => "Left pointer (standard)",
            Self::LeftSide => "Left side resize",
            Self::LeftTee => "Left T shape",
            Self::Leftbutton => "Left button",
            Self::LlAngle => "Lower-left angle",
            Self::LrAngle => "Lower-right angle",
            Self::Man => "Man figure",
            Self::Middlebutton => "Middle button",
            Self::Mouse => "Mouse",
            Self::Pencil => "Pencil",
            Self::Pirate => "Pirate",
            Self::Plus => "Plus sign",
            Self::QuestionArrow => "Question arrow",
            Self::RightPtr => "Right pointer",
            Self::RightSide => "Right side resize",
            Self::RightTee => "Right T shape",
            Self::Rightbutton => "Right button",
            Self::RtlLogo => "RTL logo",
            Self::Sailboat => "Sailboat",
            Self::SbDownArrow => "Scrollbar down arrow",
            Self::SbHDoubleArrow => "Horizontal double arrow",
            Self::SbLeftArrow => "Scrollbar left arrow",
            Self::SbRightArrow => "Scrollbar right arrow",
            Self::SbUpArrow => "Scrollbar up arrow",
            Self::SbVDoubleArrow => "Vertical double arrow",
            Self::Shuttle => "Shuttle",
            Self::Sizing => "Sizing",
            Self::Spider => "Spider",
            Self::Spraycan => "Spray can",
            Self::Star => "Star",
            Self::Target => "Target",
            Self::Tcross => "T cross",
            Self::TopLeftArrow => "Top-left arrow",
            Self::TopLeftCorner => "Top-left corner resize",
            Self::TopRightCorner => "Top-right corner resize",
            Self::TopSide => "Top side resize",
            Self::TopTee => "Top T shape",
            Self::Trek => "Star Trek",
            Self::UlAngle => "Upper-left angle",
            Self::Umbrella => "Umbrella",
            Self::UrAngle => "Upper-right angle",
            Self::Watch => "Watch/waiting",
            Self::Xterm => "Text cursor",
        }
    }

    /// 获取常用光标列表
    pub fn common_cursors() -> &'static [StandardCursor] {
        &[
            Self::LeftPtr,           // 标准箭头
            Self::Hand1,             // 手型光标
            Self::Xterm,             // 文本光标
            Self::Watch,             // 等待光标
            Self::Crosshair,         // 十字线
            Self::Fleur,             // 四向移动
            Self::SbHDoubleArrow,    // 水平调整
            Self::SbVDoubleArrow,    // 垂直调整
            Self::TopLeftCorner,     // 左上角调整
            Self::TopRightCorner,    // 右上角调整
            Self::BottomLeftCorner,  // 左下角调整
            Self::BottomRightCorner, // 右下角调整
            Self::Sizing,            // 大小调整
        ]
    }

    /// 获取所有光标列表
    pub fn all_cursors() -> &'static [StandardCursor] {
        &[
            Self::XCursor,
            Self::Arrow,
            Self::BasedArrowDown,
            Self::BasedArrowUp,
            Self::Boat,
            Self::Bogosity,
            Self::BottomLeftCorner,
            Self::BottomRightCorner,
            Self::BottomSide,
            Self::BottomTee,
            Self::BoxSpiral,
            Self::CenterPtr,
            Self::Circle,
            Self::Clock,
            Self::CoffeeMug,
            Self::Cross,
            Self::CrossReverse,
            Self::Crosshair,
            Self::DiamondCross,
            Self::Dot,
            Self::Dotbox,
            Self::DoubleArrow,
            Self::DraftLarge,
            Self::DraftSmall,
            Self::DrapedBox,
            Self::Exchange,
            Self::Fleur,
            Self::Gobbler,
            Self::Gumby,
            Self::Hand1,
            Self::Hand2,
            Self::Heart,
            Self::Icon,
            Self::IronCross,
            Self::LeftPtr,
            Self::LeftSide,
            Self::LeftTee,
            Self::Leftbutton,
            Self::LlAngle,
            Self::LrAngle,
            Self::Man,
            Self::Middlebutton,
            Self::Mouse,
            Self::Pencil,
            Self::Pirate,
            Self::Plus,
            Self::QuestionArrow,
            Self::RightPtr,
            Self::RightSide,
            Self::RightTee,
            Self::Rightbutton,
            Self::RtlLogo,
            Self::Sailboat,
            Self::SbDownArrow,
            Self::SbHDoubleArrow,
            Self::SbLeftArrow,
            Self::SbRightArrow,
            Self::SbUpArrow,
            Self::SbVDoubleArrow,
            Self::Shuttle,
            Self::Sizing,
            Self::Spider,
            Self::Spraycan,
            Self::Star,
            Self::Target,
            Self::Tcross,
            Self::TopLeftArrow,
            Self::TopLeftCorner,
            Self::TopRightCorner,
            Self::TopSide,
            Self::TopTee,
            Self::Trek,
            Self::UlAngle,
            Self::Umbrella,
            Self::UrAngle,
            Self::Watch,
            Self::Xterm,
        ]
    }
}

// 基本使用
#[allow(dead_code)]
fn create_standard_cursors(conn: &impl Connection) -> Result<(), Box<dyn std::error::Error>> {
    // 打开光标字体
    let cursor_font = conn.generate_id()?;
    conn.open_font(cursor_font, b"cursor")?;

    // 创建各种光标
    let arrow_cursor = StandardCursor::LeftPtr.create(conn, cursor_font)?;
    let hand_cursor = StandardCursor::Hand1.create(conn, cursor_font)?;
    let text_cursor = StandardCursor::Xterm.create(conn, cursor_font)?;
    let wait_cursor = StandardCursor::Watch.create(conn, cursor_font)?;
    let crosshair_cursor = StandardCursor::Crosshair.create(conn, cursor_font)?;

    println!("Created cursors successfully!");

    // 清理资源
    conn.free_cursor(arrow_cursor)?;
    conn.free_cursor(hand_cursor)?;
    conn.free_cursor(text_cursor)?;
    conn.free_cursor(wait_cursor)?;
    conn.free_cursor(crosshair_cursor)?;
    conn.close_font(cursor_font)?;

    Ok(())
}

// 创建彩色光标
#[allow(dead_code)]
fn create_colored_cursors(conn: &impl Connection) -> Result<(), Box<dyn std::error::Error>> {
    let cursor_font = conn.generate_id()?;
    conn.open_font(cursor_font, b"cursor")?;

    // 红色箭头光标
    let red_arrow = StandardCursor::LeftPtr.create_colored(
        conn,
        cursor_font,
        65535,
        0,
        0, // 红色前景
        65535,
        65535,
        65535, // 白色背景
    )?;

    // 蓝色手型光标
    let blue_hand = StandardCursor::Hand1.create_colored(
        conn,
        cursor_font,
        0,
        0,
        65535, // 蓝色前景
        65535,
        65535,
        65535, // 白色背景
    )?;

    // 清理
    conn.free_cursor(red_arrow)?;
    conn.free_cursor(blue_hand)?;
    conn.close_font(cursor_font)?;

    Ok(())
}

// 光标管理器
pub struct CursorManager {
    font: Font,
    cursors: std::collections::HashMap<StandardCursor, Cursor>,
}

impl CursorManager {
    pub fn new(conn: &impl Connection) -> Result<Self, Box<dyn std::error::Error>> {
        let font = conn.generate_id()?;
        conn.open_font(font, b"cursor")?;

        let mut cursors = std::collections::HashMap::new();

        // 预创建常用光标
        for &cursor_type in StandardCursor::common_cursors() {
            let cursor = cursor_type.create(conn, font)?;
            cursors.insert(cursor_type, cursor);
        }

        Ok(CursorManager { font, cursors })
    }

    pub fn get_cursor(
        &mut self,
        conn: &impl Connection,
        cursor_type: StandardCursor,
    ) -> Result<Cursor, Box<dyn std::error::Error>> {
        if let Some(&cursor) = self.cursors.get(&cursor_type) {
            Ok(cursor)
        } else {
            let cursor = cursor_type.create(conn, self.font)?;
            self.cursors.insert(cursor_type, cursor);
            Ok(cursor)
        }
    }

    pub fn apply_cursor(
        &mut self,
        conn: &impl Connection,
        window: Window,
        cursor_type: StandardCursor,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let cursor = self.get_cursor(conn, cursor_type)?;
        conn.change_window_attributes(window, &ChangeWindowAttributesAux::new().cursor(cursor))?;
        Ok(())
    }

    pub fn cleanup(&self, conn: &impl Connection) -> Result<(), Box<dyn std::error::Error>> {
        for &cursor in self.cursors.values() {
            conn.free_cursor(cursor)?;
        }
        conn.close_font(self.font)?;
        Ok(())
    }
}

// 使用示例
#[allow(dead_code)]
fn example_usage() -> Result<(), Box<dyn std::error::Error>> {
    let (conn, _screen_num) = x11rb::connect(None)?;
    let mut cursor_manager = CursorManager::new(&conn)?;

    // 假设有一些窗口
    let main_window = conn.generate_id()?;
    let button_window = conn.generate_id()?;
    let text_window = conn.generate_id()?;

    // 应用不同的光标
    cursor_manager.apply_cursor(&conn, main_window, StandardCursor::LeftPtr)?;
    cursor_manager.apply_cursor(&conn, button_window, StandardCursor::Hand1)?;
    cursor_manager.apply_cursor(&conn, text_window, StandardCursor::Xterm)?;

    // 程序结束时清理
    cursor_manager.cleanup(&conn)?;

    Ok(())
}

// 测试所有光标
#[allow(dead_code)]
pub fn test_all_cursors(conn: &impl Connection) -> Result<(), Box<dyn std::error::Error>> {
    let cursor_font = conn.generate_id()?;
    conn.open_font(cursor_font, b"cursor")?;

    println!("Testing all standard cursors:");

    for &cursor_type in StandardCursor::all_cursors() {
        match cursor_type.create(conn, cursor_font) {
            Ok(cursor) => {
                println!("✓ {:?}: {}", cursor_type, cursor_type.description());
                conn.free_cursor(cursor)?;
            }
            Err(e) => {
                println!("✗ {:?}: Failed - {}", cursor_type, e);
            }
        }
    }

    conn.close_font(cursor_font)?;
    Ok(())
}

use crate::config::CONFIG;
use std::collections::HashMap;

/// ARGB颜色结构，支持Alpha通道
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArgbColor {
    pub value: u32, // ARGB格式: 0xAARRGGBB
}

impl ArgbColor {
    /// 从ARGB分量创建颜色
    pub fn new(alpha: u8, red: u8, green: u8, blue: u8) -> Self {
        let value =
            ((alpha as u32) << 24) | ((red as u32) << 16) | ((green as u32) << 8) | (blue as u32);
        Self { value }
    }

    /// 从RGB创建不透明颜色
    pub fn from_rgb(red: u8, green: u8, blue: u8) -> Self {
        Self::new(255, red, green, blue)
    }

    /// 从十六进制字符串创建颜色
    pub fn from_hex(hex: &str, alpha: u8) -> Result<Self, Box<dyn std::error::Error>> {
        let (r, g, b) = parse_hex_color(hex)?;
        Ok(Self::new(alpha, r, g, b))
    }

    /// 提取ARGB分量
    pub fn components(&self) -> (u8, u8, u8, u8) {
        let alpha = (self.value >> 24) as u8;
        let red = (self.value >> 16) as u8;
        let green = (self.value >> 8) as u8;
        let blue = self.value as u8;
        (alpha, red, green, blue)
    }

    /// 获取RGB分量（不包含alpha）
    pub fn rgb(&self) -> (u8, u8, u8) {
        let (_, r, g, b) = self.components();
        (r, g, b)
    }

    /// 获取alpha值
    pub fn alpha(&self) -> u8 {
        (self.value >> 24) as u8
    }

    /// 设置alpha值
    pub fn with_alpha(&self, alpha: u8) -> Self {
        let (_, r, g, b) = self.components();
        Self::new(alpha, r, g, b)
    }

    /// 转换为浮点RGBA（用于Cairo等）
    pub fn to_rgba_f64(&self) -> (f64, f64, f64, f64) {
        let (a, r, g, b) = self.components();
        (
            r as f64 / 255.0,
            g as f64 / 255.0,
            b as f64 / 255.0,
            a as f64 / 255.0,
        )
    }

    /// 获取X11像素值（去除alpha）
    pub fn to_x11_pixel(&self) -> u32 {
        self.value & 0x00FFFFFF
    }
}

/// 颜色方案
#[derive(Debug, Clone)]
pub struct ColorScheme {
    pub fg: ArgbColor,     // 前景色
    pub bg: ArgbColor,     // 背景色
    pub border: ArgbColor, // 边框色
}

impl ColorScheme {
    /// 创建新的颜色方案
    pub fn new(fg: ArgbColor, bg: ArgbColor, border: ArgbColor) -> Self {
        Self { fg, bg, border }
    }

    /// 从十六进制字符串创建颜色方案
    pub fn from_hex(
        fg_hex: &str,
        bg_hex: &str,
        border_hex: &str,
        alpha: u8,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self::new(
            ArgbColor::from_hex(fg_hex, alpha)?,
            ArgbColor::from_hex(bg_hex, alpha)?,
            ArgbColor::from_hex(border_hex, alpha)?,
        ))
    }

    /// 获取前景色
    pub fn foreground(&self) -> ArgbColor {
        self.fg
    }

    /// 获取背景色
    pub fn background(&self) -> ArgbColor {
        self.bg
    }

    /// 获取边框色
    pub fn border_color(&self) -> ArgbColor {
        self.border
    }
}

/// 方案类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SchemeType {
    Norm = 0,    // 普通状态
    Sel = 1,     // 选中状态
    Urgent = 2,  // 紧急状态
    Warning = 3, // 警告状态
    Error = 4,   // 错误状态
}

/// 主题管理器
#[derive(Debug, Clone)]
pub struct ThemeManager {
    schemes: HashMap<SchemeType, ColorScheme>,
    x11_color_cache: HashMap<u32, u32>, // ARGB -> X11 pixel映射缓存
}

impl ThemeManager {
    /// 创建新的主题管理器
    pub fn new() -> Self {
        Self {
            schemes: HashMap::new(),
            x11_color_cache: HashMap::new(),
        }
    }

    /// 创建默认主题
    pub fn create_default<C: Connection>(
        conn: &C,
        screen: &Screen,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut theme = Self::new();

        // 普通状态 - 深色主题
        let normal = ColorScheme::from_hex(
            "#E0E0E0", // 浅灰前景
            "#2E2E2E", // 深灰背景
            "#404040", // 中灰边框
            255,       // 不透明
        )?;

        // 选中状态 - 蓝色主题
        let selected = ColorScheme::from_hex(
            "#FFFFFF", // 白色前景
            "#007ACC", // 蓝色背景
            "#005A9E", // 深蓝边框
            255,       // 不透明
        )?;

        // 紧急状态 - 红色主题
        let urgent = ColorScheme::from_hex(
            "#FFFFFF", // 白色前景
            "#DC3545", // 红色背景
            "#C82333", // 深红边框
            255,       // 不透明
        )?;

        // 警告状态 - 黄色主题
        let warning = ColorScheme::from_hex(
            "#000000", // 黑色前景
            "#FFC107", // 黄色背景
            "#E0A800", // 深黄边框
            255,       // 不透明
        )?;

        // 错误状态 - 深红主题
        let error = ColorScheme::from_hex(
            "#FFFFFF", // 白色前景
            "#8B0000", // 深红背景
            "#660000", // 更深红边框
            255,       // 不透明
        )?;

        theme.set_scheme(SchemeType::Norm, normal);
        theme.set_scheme(SchemeType::Sel, selected);
        theme.set_scheme(SchemeType::Urgent, urgent);
        theme.set_scheme(SchemeType::Warning, warning);
        theme.set_scheme(SchemeType::Error, error);

        // 预分配X11颜色
        theme.allocate_x11_colors(conn, screen.default_colormap)?;

        Ok(theme)
    }

    /// 从配置创建主题
    pub fn create_from_config<C: Connection>(
        conn: &C,
        screen: &Screen,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut theme = Self::new();
        let colors = CONFIG.colors();

        // 从配置创建各种状态的颜色方案
        let normal = ColorScheme::new(
            ArgbColor::from_hex(&colors.dark_sea_green1, colors.opaque)?,
            ArgbColor::from_hex(&colors.light_sky_blue1, colors.opaque)?,
            ArgbColor::from_hex(&colors.light_sky_blue1, colors.opaque)?,
        );

        let selected = ColorScheme::new(
            ArgbColor::from_hex(&colors.dark_sea_green2, colors.opaque)?,
            ArgbColor::from_hex(&colors.pale_turquoise1, colors.opaque)?,
            ArgbColor::from_hex(&colors.cyan, colors.opaque)?,
        );

        theme.set_scheme(SchemeType::Norm, normal);
        theme.set_scheme(SchemeType::Sel, selected);

        // 分配X11颜色
        theme.allocate_x11_colors(conn, screen.default_colormap)?;

        Ok(theme)
    }

    /// 设置颜色方案
    pub fn set_scheme(&mut self, scheme_type: SchemeType, scheme: ColorScheme) {
        self.schemes.insert(scheme_type, scheme);
    }

    /// 获取颜色方案
    pub fn get_scheme(&self, scheme_type: SchemeType) -> Option<&ColorScheme> {
        self.schemes.get(&scheme_type)
    }

    /// 获取可变颜色方案
    pub fn get_scheme_mut(&mut self, scheme_type: SchemeType) -> Option<&mut ColorScheme> {
        self.schemes.get_mut(&scheme_type)
    }

    /// 获取前景色
    pub fn get_fg(&self, scheme_type: SchemeType) -> Option<ArgbColor> {
        self.get_scheme(scheme_type).map(|s| s.foreground())
    }

    /// 获取背景色
    pub fn get_bg(&self, scheme_type: SchemeType) -> Option<ArgbColor> {
        self.get_scheme(scheme_type).map(|s| s.background())
    }

    /// 获取边框色
    pub fn get_border(&self, scheme_type: SchemeType) -> Option<ArgbColor> {
        self.get_scheme(scheme_type).map(|s| s.border_color())
    }

    /// 获取X11像素值
    pub fn get_x11_pixel(&self, color: ArgbColor) -> Option<u32> {
        self.x11_color_cache.get(&color.value).copied()
    }

    /// 分配X11颜色
    pub fn allocate_x11_colors<C: Connection>(
        &mut self,
        conn: &C,
        colormap: Colormap,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut colors_to_allocate = Vec::new();

        // 收集所有需要分配的颜色
        for scheme in self.schemes.values() {
            colors_to_allocate.push(scheme.fg);
            colors_to_allocate.push(scheme.bg);
            colors_to_allocate.push(scheme.border);
        }

        // 去重
        colors_to_allocate.sort_by_key(|c| c.value);
        colors_to_allocate.dedup();

        // 分配颜色
        for color in colors_to_allocate {
            let pixel = self.allocate_single_color(conn, colormap, color)?;
            self.x11_color_cache.insert(color.value, pixel);
        }

        Ok(())
    }

    /// 分配单个颜色
    fn allocate_single_color<C: Connection>(
        &self,
        conn: &C,
        colormap: Colormap,
        color: ArgbColor,
    ) -> Result<u32, Box<dyn std::error::Error>> {
        let (_, r, g, b) = color.components();

        let reply = conn
            .alloc_color(colormap, (r as u16) << 8, (g as u16) << 8, (b as u16) << 8)?
            .reply()?;

        Ok(reply.pixel)
    }

    /// 释放X11颜色
    pub fn free_x11_colors<C: Connection>(
        &mut self,
        conn: &C,
        colormap: Colormap,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let pixels: Vec<u32> = self.x11_color_cache.values().copied().collect();

        if !pixels.is_empty() {
            conn.free_colors(colormap, 0, &pixels)?;
            self.x11_color_cache.clear();
        }

        Ok(())
    }
}

/// 绘图辅助功能
impl ThemeManager {
    /// 为Cairo设置颜色
    #[cfg(feature = "cairo")]
    pub fn set_cairo_source(&self, ctx: &cairo::Context, color: ArgbColor) {
        let (r, g, b, a) = color.to_rgba_f64();
        ctx.set_source_rgba(r, g, b, a);
    }

    /// 绘制背景（Cairo）
    #[cfg(feature = "cairo")]
    pub fn draw_cairo_background(
        &self,
        ctx: &cairo::Context,
        scheme_type: SchemeType,
        width: f64,
        height: f64,
    ) {
        if let Some(bg_color) = self.get_bg(scheme_type) {
            self.set_cairo_source(ctx, bg_color);
            ctx.rectangle(0.0, 0.0, width, height);
            let _ = ctx.fill();
        }
    }

    /// 绘制边框（Cairo）
    #[cfg(feature = "cairo")]
    pub fn draw_cairo_border(
        &self,
        ctx: &cairo::Context,
        scheme_type: SchemeType,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        border_width: f64,
    ) {
        if let Some(border_color) = self.get_border(scheme_type) {
            self.set_cairo_source(ctx, border_color);
            ctx.set_line_width(border_width);
            ctx.rectangle(x, y, width, height);
            let _ = ctx.stroke();
        }
    }
}

/// 辅助函数
fn parse_hex_color(hex: &str) -> Result<(u8, u8, u8), Box<dyn std::error::Error>> {
    let hex = if hex.starts_with('#') { &hex[1..] } else { hex };

    match hex.len() {
        3 => {
            // #RGB -> #RRGGBB
            let r = u8::from_str_radix(&hex[0..1].repeat(2), 16)?;
            let g = u8::from_str_radix(&hex[1..2].repeat(2), 16)?;
            let b = u8::from_str_radix(&hex[2..3].repeat(2), 16)?;
            Ok((r, g, b))
        }
        6 => {
            // #RRGGBB
            let r = u8::from_str_radix(&hex[0..2], 16)?;
            let g = u8::from_str_radix(&hex[2..4], 16)?;
            let b = u8::from_str_radix(&hex[4..6], 16)?;
            Ok((r, g, b))
        }
        _ => Err("Invalid hex color format".into()),
    }
}

/// 预定义颜色常量
impl ArgbColor {
    pub const TRANSPARENT: ArgbColor = ArgbColor { value: 0x00000000 };
    pub const BLACK: ArgbColor = ArgbColor { value: 0xFF000000 };
    pub const WHITE: ArgbColor = ArgbColor { value: 0xFFFFFFFF };
    pub const RED: ArgbColor = ArgbColor { value: 0xFFFF0000 };
    pub const GREEN: ArgbColor = ArgbColor { value: 0xFF00FF00 };
    pub const BLUE: ArgbColor = ArgbColor { value: 0xFF0000FF };
    pub const YELLOW: ArgbColor = ArgbColor { value: 0xFFFFFF00 };
    pub const CYAN: ArgbColor = ArgbColor { value: 0xFF00FFFF };
    pub const MAGENTA: ArgbColor = ArgbColor { value: 0xFFFF00FF };
}

/// 使用示例和测试
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_argb_color() {
        let color = ArgbColor::new(128, 255, 0, 0); // 半透明红色
        assert_eq!(color.value, 0x80FF0000);

        let (a, r, g, b) = color.components();
        assert_eq!((a, r, g, b), (128, 255, 0, 0));
    }

    #[test]
    fn test_hex_parsing() {
        let color = ArgbColor::from_hex("#FF0000", 255).unwrap();
        assert_eq!(color.rgb(), (255, 0, 0));

        let color = ArgbColor::from_hex("F00", 128).unwrap();
        assert_eq!(color.components(), (128, 255, 0, 0));
    }

    #[test]
    fn test_color_scheme() {
        let scheme = ColorScheme::from_hex("#000000", "#FFFFFF", "#808080", 255).unwrap();
        assert_eq!(scheme.foreground().rgb(), (0, 0, 0));
        assert_eq!(scheme.background().rgb(), (255, 255, 255));
        assert_eq!(scheme.border_color().rgb(), (128, 128, 128));
    }
}
