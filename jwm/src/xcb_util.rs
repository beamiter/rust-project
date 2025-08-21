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
        _NET_WM_STATE,
        _NET_SUPPORTING_WM_CHECK,
        _NET_WM_STATE_FULLSCREEN,
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_DIALOG,
        _NET_CLIENT_LIST,
        _NET_CLIENT_INFO,

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

use x11::xft::XftColor;

use crate::config::CONFIG;
#[repr(C)]
#[derive(Debug, Clone)]
pub struct ColorScheme {
    pub fg: XftColor,     // 前景色
    pub bg: XftColor,     // 背景色
    pub border: XftColor, // 边框色
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemeType {
    Norm = 0, // 普通状态
    Sel = 1,  // 选中状态
}

#[repr(C)]
#[derive(Debug, Clone)]
pub struct ThemeManager {
    pub normal: ColorScheme, // 普通状态的颜色方案
    pub selected: ColorScheme,  // 选中状态的颜色方案
}

#[allow(dead_code)]
impl ColorScheme {
    /// 创建新的颜色方案
    pub fn new(fg: XftColor, bg: XftColor, border: XftColor) -> Self {
        Self { fg, bg, border }
    }

    /// 获取前景色
    pub fn foreground(&self) -> &XftColor {
        &self.fg
    }

    /// 获取背景色
    pub fn background(&self) -> &XftColor {
        &self.bg
    }

    /// 获取边框色
    pub fn border_color(&self) -> &XftColor {
        &self.border
    }

    /// 设置前景色
    pub fn set_foreground(&mut self, color: XftColor) {
        self.fg = color;
    }

    /// 设置背景色
    pub fn set_background(&mut self, color: XftColor) {
        self.bg = color;
    }

    /// 设置边框色
    pub fn set_border(&mut self, color: XftColor) {
        self.border = color;
    }
}

#[allow(dead_code)]
impl ThemeManager {
    /// 创建新的主题管理器
    pub fn new(norm: ColorScheme, sel: ColorScheme) -> Self {
        Self { normal: norm, selected: sel }
    }

    pub fn create_aux() -> Self {
        ThemeManager::new(
            ColorScheme::new(
                Self::drw_clr_create_from_hex(
                    &CONFIG.colors().dark_sea_green1,
                    CONFIG.colors().opaque,
                )
                .unwrap(),
                Self::drw_clr_create_from_hex(
                    &CONFIG.colors().light_sky_blue1,
                    CONFIG.colors().opaque,
                )
                .unwrap(),
                Self::drw_clr_create_from_hex(
                    &CONFIG.colors().light_sky_blue1,
                    CONFIG.colors().opaque,
                )
                .unwrap(),
            ),
            ColorScheme::new(
                Self::drw_clr_create_from_hex(
                    &CONFIG.colors().dark_sea_green2,
                    CONFIG.colors().opaque,
                )
                .unwrap(),
                Self::drw_clr_create_from_hex(
                    &CONFIG.colors().pale_turquoise1,
                    CONFIG.colors().opaque,
                )
                .unwrap(),
                Self::drw_clr_create_from_hex(&CONFIG.colors().cyan, CONFIG.colors().opaque)
                    .unwrap(),
            ),
        )
    }

    /// 根据方案类型获取颜色方案
    pub fn get_scheme(&self, scheme_type: SchemeType) -> &ColorScheme {
        match scheme_type {
            SchemeType::Norm => &self.normal,
            SchemeType::Sel => &self.selected,
        }
    }

    /// 获取可变颜色方案
    pub fn get_scheme_mut(&mut self, scheme_type: SchemeType) -> &mut ColorScheme {
        match scheme_type {
            SchemeType::Norm => &mut self.normal,
            SchemeType::Sel => &mut self.selected,
        }
    }

    /// 获取指定方案的前景色
    pub fn get_fg(&self, scheme_type: SchemeType) -> &XftColor {
        self.get_scheme(scheme_type).foreground()
    }

    /// 获取指定方案的背景色
    pub fn get_bg(&self, scheme_type: SchemeType) -> &XftColor {
        self.get_scheme(scheme_type).background()
    }

    /// 获取指定方案的边框色
    pub fn get_border(&self, scheme_type: SchemeType) -> &XftColor {
        self.get_scheme(scheme_type).border_color()
    }

    /// 设置整个颜色方案
    pub fn set_scheme(&mut self, scheme_type: SchemeType, color_scheme: ColorScheme) {
        match scheme_type {
            SchemeType::Norm => self.normal = color_scheme,
            SchemeType::Sel => self.selected = color_scheme,
        }
    }

    pub fn drw_clr_create_direct(r: u8, g: u8, b: u8, alpha: u8) -> Option<XftColor> {
        unsafe {
            let mut xcolor: XftColor = std::mem::zeroed();
            // 手动构造像素值 (ARGB格式)
            xcolor.pixel =
                ((alpha as u64) << 24) | ((r as u64) << 16) | ((g as u64) << 8) | (b as u64);
            // 设置其他字段
            xcolor.color.red = (r as u16) << 8;
            xcolor.color.green = (g as u16) << 8;
            xcolor.color.blue = (b as u16) << 8;
            xcolor.color.alpha = (alpha as u16) << 8;
            Some(xcolor)
        }
    }

    pub fn drw_clr_create_from_hex(hex_color: &str, alpha: u8) -> Option<XftColor> {
        // 解析 "#ff0000" 格式
        if hex_color.starts_with('#') && hex_color.len() == 7 {
            let r = u8::from_str_radix(&hex_color[1..3], 16).ok()?;
            let g = u8::from_str_radix(&hex_color[3..5], 16).ok()?;
            let b = u8::from_str_radix(&hex_color[5..7], 16).ok()?;
            return Self::drw_clr_create_direct(r, g, b, alpha);
        }
        None
    }
}
