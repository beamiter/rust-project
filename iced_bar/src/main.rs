use ::iced_fonts::NERD_FONT_BYTES;
use iced::{
    Element, Length, color,
    widget::{
        Column, button, container, rich_text, span,
        text::{self},
    },
};
use iced_aw::{TabBar, TabLabel};

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
}

impl Default for TabBarExample {
    fn default() -> Self {
        TabBarExample::new()
    }
}

impl TabBarExample {
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
        let rich_label = rich_text([span("🌴111111111😻")]);
        let tab_button = button(container(rich_label).padding(1)).on_press(Message::ButtonPressed);
        Column::new()
            .push(
                self.tabs
                    .iter()
                    .fold(TabBar::new(Message::TabSelected), |tab_bar, tab_label| {
                        let idx = tab_bar.size();
                        tab_bar.push(idx, TabLabel::Text(tab_label.to_owned()))
                    })
                    .set_active_tab(&self.active_tab)
                    .tab_width(Length::Shrink)
                    .spacing(3.0)
                    .padding(1.0)
                    .text_size(16.0),
            )
            .push(
                // Draw underline here
                tab_button,
            )
            .into()
    }
}
