// src/backend/x11/cursor.rs
use crate::backend::api::{CursorHandle, CursorProvider};
use crate::backend::common_define::StdCursorKind;
use std::collections::HashMap;
use std::sync::Arc;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum X11StdCursor {
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

impl X11StdCursor {
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
    pub fn common_cursors() -> &'static [X11StdCursor] {
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
    pub fn all_cursors() -> &'static [X11StdCursor] {
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

pub struct X11CursorProvider<C: Connection> {
    conn: Arc<C>,
    cursor_font: Font,
    cache: HashMap<StdCursorKind, Cursor>,
}

impl<C: Connection> X11CursorProvider<C> {
    pub fn new(conn: Arc<C>) -> Result<Self, Box<dyn std::error::Error>> {
        use x11rb::protocol::xproto::ConnectionExt;
        let font = conn.generate_id()?;
        conn.open_font(font, b"cursor")?.check()?;
        Ok(Self {
            conn,
            cursor_font: font,
            cache: HashMap::new(),
        })
    }

    fn map_kind(kind: StdCursorKind) -> X11StdCursor {
        match kind {
            StdCursorKind::LeftPtr => X11StdCursor::LeftPtr,
            StdCursorKind::Hand => X11StdCursor::Hand1,
            StdCursorKind::XTerm => X11StdCursor::Xterm,
            StdCursorKind::Watch => X11StdCursor::Watch,
            StdCursorKind::Crosshair => X11StdCursor::Crosshair,
            StdCursorKind::Fleur => X11StdCursor::Fleur,
            StdCursorKind::HDoubleArrow => X11StdCursor::SbHDoubleArrow,
            StdCursorKind::VDoubleArrow => X11StdCursor::SbVDoubleArrow,
            StdCursorKind::TopLeftCorner => X11StdCursor::TopLeftCorner,
            StdCursorKind::TopRightCorner => X11StdCursor::TopRightCorner,
            StdCursorKind::BottomLeftCorner => X11StdCursor::BottomLeftCorner,
            StdCursorKind::BottomRightCorner => X11StdCursor::BottomRightCorner,
            StdCursorKind::Sizing => X11StdCursor::Sizing,
        }
    }
}

impl<C: Connection + Send + Sync + 'static> CursorProvider for X11CursorProvider<C> {
    fn preload_common(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        for kind in [
            StdCursorKind::LeftPtr,
            StdCursorKind::Hand,
            StdCursorKind::XTerm,
            StdCursorKind::Watch,
            StdCursorKind::Crosshair,
            StdCursorKind::Fleur,
            StdCursorKind::HDoubleArrow,
            StdCursorKind::VDoubleArrow,
            StdCursorKind::TopLeftCorner,
            StdCursorKind::TopRightCorner,
            StdCursorKind::BottomLeftCorner,
            StdCursorKind::BottomRightCorner,
            StdCursorKind::Sizing,
        ] {
            let _ = self.get(kind)?;
        }
        Ok(())
    }

    fn get(&mut self, kind: StdCursorKind) -> Result<CursorHandle, Box<dyn std::error::Error>> {
        if let Some(&c) = self.cache.get(&kind) {
            return Ok(CursorHandle(c as u64));
        }
        let x11_cursor = Self::map_kind(kind);
        let cursor = x11_cursor.create(&*self.conn, self.cursor_font)?;
        self.cache.insert(kind, cursor);
        Ok(CursorHandle(cursor as u64))
    }

    fn apply(
        &mut self,
        window_id: u64,
        kind: StdCursorKind,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use x11rb::protocol::xproto::ConnectionExt;
        let c = match self.get(kind) {
            Ok(h) => h.0 as u32,
            Err(e) => return Err(e),
        };
        (*self.conn).change_window_attributes(
            window_id as u32,
            &ChangeWindowAttributesAux::new().cursor(c),
        )?;
        Ok(())
    }

    fn cleanup(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        use x11rb::protocol::xproto::ConnectionExt;
        for &cursor in self.cache.values() {
            let _ = (*self.conn).free_cursor(cursor);
        }
        let _ = (*self.conn).close_font(self.cursor_font);
        Ok(())
    }
}
