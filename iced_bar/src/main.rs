use iced::{
    Background, Border, Color, Element, Length, Padding, Theme,
    border::Radius,
    color,
    widget::{Column, Row, button, container, text},
};
use iced_aw::{TabBar, TabLabel};
use iced_fonts::NERD_FONT_BYTES;

fn main() -> iced::Result {
    iced::application("iced_bar", TabBarExample::update, TabBarExample::view)
        .font(NERD_FONT_BYTES)
        .run()
}

#[derive(Debug, Clone)]
enum Message {
    TabSelected(usize),
    ButtonPressed,
    Others,
}

#[derive(Debug)]
struct TabBarExample {
    active_tab: usize,
    tabs: Vec<String>,
    tab_colors: Vec<Color>,
}

impl Default for TabBarExample {
    fn default() -> Self {
        TabBarExample::new()
    }
}

impl TabBarExample {
    const DEFAULT_COLOR: Color = color!(0x666666);
    const TAB_WIDTH: f32 = 40.0;
    const TAB_SPACING: f32 = 3.0;
    const UNDERLINE_WIDTH: f32 = 30.0;

    fn new() -> Self {
        Self {
            active_tab: 0,
            tabs: vec![
                "🍜".to_string(),
                "🎨".to_string(),
                "🍀".to_string(),
                "🧿".to_string(),
                "🌟".to_string(),
                "🐐".to_string(),
                "🏆".to_string(),
                "🕊️".to_string(),
                "🏡".to_string(),
            ],
            tab_colors: vec![
                color!(0xFF6B6B), // 红色
                color!(0x4ECDC4), // 青色
                color!(0x45B7D1), // 蓝色
                color!(0x96CEB4), // 绿色
                color!(0xFECA57), // 黄色
                color!(0xFF9FF3), // 粉色
                color!(0x54A0FF), // 淡蓝色
                color!(0x5F27CD), // 紫色
                color!(0x00D2D3), // 青绿色
            ],
        }
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::TabSelected(index) => {
                println!("Tab selected: {}", index);
                self.active_tab = index
            }
            Message::ButtonPressed => {
                println!("ButtonPressed");
            }
            _ => {
                println!("others");
            }
        }
    }

    fn view(&self) -> Element<Message> {
        // 使用固定宽度的TabBar
        let tab_bar = self
            .tabs
            .iter()
            .fold(TabBar::new(Message::TabSelected), |tab_bar, tab_label| {
                let idx = tab_bar.size();
                tab_bar.push(idx, TabLabel::Text(tab_label.to_owned()))
            })
            .set_active_tab(&self.active_tab)
            .tab_width(Length::Fixed(Self::TAB_WIDTH))
            .spacing(Self::TAB_SPACING)
            .padding(1.0)
            .text_size(16.0);

        // 创建下划线行 - 修正版
        let mut underline_row = Row::new().spacing(Self::TAB_SPACING);

        for (index, _) in self.tabs.iter().enumerate() {
            let is_active = index == self.active_tab;
            let tab_color = self.tab_colors.get(index).unwrap_or(&Self::DEFAULT_COLOR);

            // 创建下划线
            let underline = if is_active {
                // 激活状态：显示彩色下划线
                container(
                    container(text(" ")) // 使用空格而不是空字符串
                        .width(Length::Fixed(Self::UNDERLINE_WIDTH))
                        .height(Length::Fixed(3.0))
                        .style(move |_theme: &Theme| container::Style {
                            background: Some(Background::Color(*tab_color)),
                            border: Border::default(),
                            ..Default::default()
                        }),
                )
                .center_x(Length::Fixed(Self::TAB_WIDTH))
            } else {
                // 非激活状态：透明占位符
                container(text(" "))
                    .width(Length::Fixed(Self::TAB_WIDTH))
                    .height(Length::Fixed(3.0))
            };

            underline_row = underline_row.push(underline);
        }

        let padding = Padding {
            top: 10.0,
            ..Default::default()
        };
        Column::new()
            .push(tab_bar)
            .push(underline_row)
            .push(
                container(text(format!("chosen: Tab {}", self.active_tab)).size(18))
                    .padding(padding),
            )
            .spacing(1)
            .padding(10)
            .into()
    }
}
