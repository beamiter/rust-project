
use std::f32::consts::PI;

use iced::Color;
use iced::alignment;
use iced::mouse;
use iced::time::{self, milliseconds};
use iced::widget::canvas::path::Arc;
use iced::widget::canvas::{Cache, Geometry, LineCap, Path, Stroke, stroke};
use iced::widget::{canvas, container, text};
use iced::{
    Degrees, Element, Fill, Font, Point, Radians, Rectangle, Renderer, Size,
    Subscription, Theme, Vector,
};

pub fn main() -> iced::Result {
    tracing_subscriber::fmt::init();

    iced::application(Clock::new, Clock::update, Clock::view)
        .subscription(Clock::subscription)
        .theme(Clock::theme)
        .run()
}

struct Clock {
    now: chrono::DateTime<chrono::Local>,
    clock: Cache,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Tick(chrono::DateTime<chrono::Local>),
}

impl Clock {
    fn new() -> Self {
        Self {
            now: chrono::offset::Local::now(),
            clock: Cache::default(),
        }
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::Tick(local_time) => {
                let now = local_time;

                if now != self.now {
                    self.now = now;
                    self.clock.clear();
                }
            }
        }
    }

    fn view(&self) -> Element<Message> {
        let canvas = canvas(self as &Self).width(Fill).height(Fill);

        container(canvas).padding(2).into()
    }

    fn subscription(&self) -> Subscription<Message> {
        time::every(milliseconds(500))
            .map(|_| Message::Tick(chrono::offset::Local::now()))
    }

    fn theme(&self) -> Theme {
        Theme::ALL[(self.now.timestamp() as usize / 10) % Theme::ALL.len()]
            .clone()
    }
}

impl<Message> canvas::Program<Message> for Clock {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        // let clock = self.clock.draw(renderer, bounds.size(), |frame| {
        //     let palette = theme.extended_palette();
        //
        //     let center = frame.center();
        //     let radius = frame.width().min(frame.height()) / 2.0;
        //
        //     let background = Path::circle(center, radius);
        //     frame.fill(&background, palette.secondary.strong.color);
        //
        //     // frame.translate(Vector::new(center.x, center.y));
        // });

        // 使用缓存进行绘制

        let sector = self.clock.draw(renderer, bounds.size(), |frame| {
            // 1. 定义扇形的几何属性

            let center = frame.center();

            let radius = frame.width().min(frame.height()) * 0.4;

            let start_angle_deg = 0.0;

            let end_angle_deg = 120.0;

            // 将角度转换为弧度

            let start_angle_rad = Radians::from(Degrees(start_angle_deg));

            let end_angle_rad = Radians::from(Degrees(end_angle_deg));

            // 2. 构建扇形路径 (Path) - [核心修正部分]

            let sector_path = Path::new(|builder| {
                // 步骤 a: 将画笔移动到圆心

                builder.move_to(center);

                // 步骤 b: 画第一条半径。我们需要计算出圆弧的起点坐标，然后画一条直线过去。

                let start_point = Point {
                    x: center.x + radius * start_angle_rad.0.cos(),

                    y: center.y + radius * start_angle_rad.0.sin(),
                };

                builder.line_to(start_point);

                // 步骤 c: 绘制圆弧。此时画笔位于圆弧起点，arc会从这个点开始画。

                builder.arc(Arc {
                    center,

                    radius,

                    start_angle: start_angle_rad.into(),

                    end_angle: end_angle_rad.into(),
                });

                // 步骤 d: 闭合路径。这会从圆弧的终点画一条直线回到整个路径的起点（即圆心），

                // 从而形成第二条半径。

                builder.line_to(center);
            });

            // 3. 填充路径

            let fill_color = Color::from_rgb8(0, 150, 255);

            frame.fill(&sector_path, fill_color);

            // 4. (可选) 添加描边

            let stroke = canvas::Stroke {
                style: canvas::Style::Solid(Color::BLACK),

                width: 2.0,

                ..canvas::Stroke::default()
            };

            frame.stroke(&sector_path, stroke);
        });

        // 将生成的几何图形返回给 Iced 进行渲染

        vec![sector]
    }
}
