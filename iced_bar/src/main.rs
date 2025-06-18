use ::iced_fonts::NERD_FONT_BYTES;
use iced::{
    Element, Length,
    widget::{Column, text},
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
        }
    }

    fn view(&self) -> Element<Message> {
        Column::new()
            .push(
                self.tabs
                    .iter()
                    .fold(TabBar::new(Message::TabSelected), |tab_bar, tab_label| {
                        // manually create a new index for the new tab
                        // starting from 0, when there is no tab created yet
                        let idx = tab_bar.size();
                        tab_bar.push(idx, TabLabel::Text(tab_label.to_owned()))
                    })
                    .set_active_tab(&self.active_tab)
                    // .on_close(Message::TabClosed)
                    .tab_width(Length::Shrink)
                    .spacing(3.0)
                    .padding(1.0)
                    .text_size(16.0),
            )
            .push(
                // Draw underline here
                text("1"),
            )
            .into()
    }
}
