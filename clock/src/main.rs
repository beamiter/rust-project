use iced::Color;
use iced::Length;
use iced::alignment;
use iced::color;
use iced::gradient;
use iced::mouse;
use iced::time::Duration;
use iced::time::{self};
use iced::widget::Column;
use iced::widget::Row;
use iced::widget::Scrollable;
use iced::widget::Space;
use iced::widget::button;
use iced::widget::canvas::{Cache, Geometry, LineCap, Path, Stroke, stroke};
use iced::widget::lazy;
use iced::widget::mouse_area;
use iced::widget::rich_text;
use iced::widget::row;
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::span;
use iced::widget::{canvas, container, text};
use iced::{
    Degrees, Element, Fill, Font, Point, Radians, Rectangle, Renderer, Size, Subscription, Theme,
    Vector,
};
use iced_aw::TabBar;
use iced_aw::TabLabel;

use log::{error, info, warn};

pub fn main() -> iced::Result {
    tracing_subscriber::fmt::init();

    iced::application("iced_bar", Clock::update, Clock::view)
        .subscription(Clock::subscription)
        .theme(Clock::theme)
        .run()
}

struct Clock {
    now: chrono::DateTime<chrono::Local>,
    clock: Cache,

    active_tab: usize,
    tabs: Vec<String>,
    tab_colors: Vec<Color>,
    layout_symbol: String,
    message_count: u32,
    scale_factor_string: String,
    is_hovered: bool,
    formated_now: String,
    monitor_num: u8,
    show_seconds: bool,
}

impl Default for Clock {
    fn default() -> Self {
        Clock::new()
    }
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Tick(chrono::DateTime<chrono::Local>),
    TabSelected(usize),
    LayoutClicked(u32),
    ShowSecondsToggle,
    MouseEnter,
    MouseExit,
    LeftClick,
    RightClick,
}

impl Clock {
    const DEFAULT_COLOR: Color = color!(0x666666);
    const TAB_WIDTH: f32 = 32.0;
    const TAB_HEIGHT: f32 = 32.0;
    const TAB_SPACING: f32 = 1.0;
    const UNDERLINE_WIDTH: f32 = 28.0;
    const TEXT_SIZE: f32 = 18.0;

    fn new() -> Self {
        Self {
            now: chrono::offset::Local::now(),
            clock: Cache::default(),

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
            layout_symbol: " ? ".to_string(),
            message_count: 0,
            scale_factor_string: "1.0".to_string(),
            is_hovered: false,
            formated_now: String::new(),
            monitor_num: 0,
            show_seconds: false,
        }
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::Tick(local_time) => {
                let now = local_time;

                self.now = now;
                self.clock.clear();
            }

            Message::TabSelected(index) => {
                info!("Tab selected: {}", index);
                self.active_tab = index;
                // 提取需要的数据，避免借用 self
                // if let Some(ref command_sender) = self.command_sender {
                //     let sender = command_sender.clone();
                //     let last_message = self.last_shared_message.clone();
                //     let active_tab = self.active_tab;
                //     return Task::perform(
                //         Self::send_tag_command_async(
                //             sender,
                //             last_message,
                //             active_tab,
                //             true,
                //         ),
                //         |_| Message::CheckSharedMessages,
                //     );
                // }
                // Task::none()
            }

            Message::LayoutClicked(layout_index) => {
                // if let Some(ref message) = self.last_shared_message {
                //     let monitor_id = message.monitor_info.monitor_num;
                //     if let Some(ref command_sender) = self.command_sender {
                //         let sender = command_sender.clone();
                //         return Task::perform(
                //             Self::send_layout_command_async(
                //                 sender,
                //                 layout_index,
                //                 monitor_id,
                //             ),
                //             |_| Message::CheckSharedMessages,
                //         );
                //     }
                // }
                // Task::none()
            }

            Message::MouseEnter => {
                self.is_hovered = true;
            }

            Message::ShowSecondsToggle => {
                self.show_seconds = !self.show_seconds;
            }

            Message::MouseExit => {
                self.is_hovered = false;
            }

            Message::LeftClick => {
                let _ = std::process::Command::new("flameshot").arg("gui").spawn();
            }

            Message::RightClick => {}
        }
    }

    fn view(&self) -> Element<Message> {
        // info!("view");
        // let work_space_row = self.view_work_space().explain(Color::from_rgb(1., 0., 1.));
        let work_space_row = self.view_work_space();

        // let under_line_row = self.view_under_line();

        Column::new()
            .padding(2)
            .spacing(2)
            .push(work_space_row)
            // .push(under_line_row)
            .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        time::every(Duration::from_millis(25)).map(|_| Message::Tick(chrono::offset::Local::now()))
    }

    fn theme(&self) -> Theme {
        Theme::ALL[(self.now.timestamp() as usize / 10) % Theme::ALL.len()].clone()
    }

    /////////////////////////////////////////////////////////
    fn monitor_num_to_icon(monitor_num: u8) -> &'static str {
        match monitor_num {
            0 => "🥇",
            1 => "🥈",
            2 => "🥉",
            _ => "?",
        }
    }

    fn view_work_space(&self) -> Element<Message> {
        // lazy template
        // let _ = lazy(&, |_| {});
        let tab_bar = lazy(&self.message_count, |_| {
            let tab_bar = self
                .tabs
                .iter()
                .fold(TabBar::new(Message::TabSelected), |tab_bar, tab_label| {
                    let idx = tab_bar.size();
                    tab_bar.push(idx, TabLabel::Text(tab_label.to_owned()))
                })
                .set_active_tab(&self.active_tab)
                .tab_width(Length::Fixed(Self::TAB_WIDTH))
                .height(Length::Fixed(Self::TAB_HEIGHT))
                .spacing(Self::TAB_SPACING)
                .padding(0.0)
                .width(Length::Shrink)
                .text_size(Self::TEXT_SIZE);
            tab_bar
        });

        let layout_text = lazy(&self.layout_symbol, |_| {
            let layout_text =
                container(rich_text([span(self.layout_symbol.clone())])).center_x(Length::Shrink);
            layout_text
        });

        let scrollable_content = lazy(&self.layout_symbol, |_| {
            let scrollable_content = Scrollable::with_direction(
                row![
                    button("[]=").on_press(Message::LayoutClicked(0)),
                    button("><>").on_press(Message::LayoutClicked(1)),
                    button("[M]").on_press(Message::LayoutClicked(2)),
                ]
                .spacing(10)
                .padding(0.0),
                Direction::Horizontal(Scrollbar::new().scroller_width(3.0).width(1.)),
            )
            .width(50.0)
            .height(Self::TAB_HEIGHT)
            .spacing(0.);
            scrollable_content
        });

        let cyan = Color::from_rgb(0.0, 1.0, 1.0); // 青色
        let dark_orange = Color::from_rgb(1.0, 0.5, 0.0); // 深橙色
        // let screenshot_text = container(text(format!(" s {} ", self.scale_factor_string)).center())
        let screenshot_text = container(text(format!(" s ")).center())
            .center_y(Length::Fill)
            .style(move |_theme: &Theme| {
                if self.is_hovered {
                    container::Style {
                        text_color: Some(dark_orange),
                        border: iced::Border {
                            radius: iced::border::radius(2.0),
                            ..Default::default()
                        },
                        background: Some(iced::Background::Color(cyan)),
                        ..Default::default()
                    }
                } else {
                    container::Style {
                        border: iced::Border {
                            color: Color::WHITE,
                            width: 0.5,
                            radius: iced::border::radius(2.0),
                        },
                        ..Default::default()
                    }
                }
            })
            .padding(0.0);

        // let time_button = button(self.formated_now.as_str())
        //     .on_press(Message::ShowSecondsToggle);
        // let clock = canvas(self as &Self)
        //     .width(Length::Fixed(Self::TAB_HEIGHT))
        //     .height(Length::Fixed(Self::TAB_HEIGHT));

        let work_space_row = Row::new()
            .push(tab_bar)
            .push(Space::with_width(3))
            .push(layout_text)
            .push(Space::with_width(3))
            .push(scrollable_content)
            .push(Space::with_width(Length::Fill))
            // .push(container(clock).style(move |_theme| {
            //     let gradient = gradient::Linear::new(0.0)
            //         .add_stop(0.0, Color::from_rgb(0.0, 1.0, 1.0))
            //         .add_stop(1.0, Color::from_rgb(1.0, 0., 0.));
            //     gradient.into()
            // }))
            .push(Space::with_width(3))
            .push(
                mouse_area(screenshot_text)
                    .on_enter(Message::MouseEnter)
                    .on_exit(Message::MouseExit)
                    .on_press(Message::LeftClick),
            )
            .push(Space::with_width(3))
            // .push(time_button)
            .push(rich_text([
                span(" "),
                span(Self::monitor_num_to_icon(self.monitor_num)),
            ]))
            .align_y(iced::Alignment::Center);

        work_space_row.into()
    }
}
