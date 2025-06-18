use iced::{
    Background, Border, Color, Element, Length, Theme,
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
            // 为每个Tab定义不同的颜色
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
        // 原来的TabBar
        let tab_bar = self
            .tabs
            .iter()
            .fold(TabBar::new(Message::TabSelected), |tab_bar, tab_label| {
                let idx = tab_bar.size();
                tab_bar.push(idx, TabLabel::Text(tab_label.to_owned()))
            })
            .set_active_tab(&self.active_tab)
            .tab_width(Length::Shrink)
            .spacing(3.0)
            .padding(1.0)
            .text_size(16.0);

        // 为每个Tab创建单独的下划线
        let underlines =
            Row::new()
                .spacing(3.0)
                .push(self.tabs.iter().enumerate().fold(
                    Row::new().spacing(3.0),
                    |row, (index, _)| {
                        let is_active = index == self.active_tab;
                        let tab_color = self.tab_colors.get(index).unwrap_or(&Self::DEFAULT_COLOR);

                        let underline = container(text(""))
                            .width(Length::Fixed(25.0)) // 短下划线宽度
                            .height(3)
                            .style(move |_theme: &Theme| container::Style {
                                background: if is_active {
                                    Some(Background::Color(*tab_color))
                                } else {
                                    Some(Background::Color(Self::DEFAULT_COLOR))
                                },
                                border: Border::default(),
                                ..Default::default()
                            });

                        row.push(underline)
                    },
                ));

        Column::new()
            .push(tab_bar)
            .push(underlines)
            .push(text(format!("chosen: Tab {}", self.active_tab)).size(18))
            .spacing(2)
            .padding(10)
            .into()
    }
}
