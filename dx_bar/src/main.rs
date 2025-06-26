use dioxus::{
    desktop::{Config, WindowBuilder},
    prelude::*,
};

fn main() {
    dioxus::LaunchBuilder::desktop()
        .with_cfg(Config::new().with_window(WindowBuilder::new().with_title("dx_bar")))
        .launch(App);
}

// 将按钮数据定义为静态常量
const BUTTONS: &[&str] = &["🔴", "🟠", "🟡", "🟢", "🔵", "🟣", "🟤", "⚪", "⚫", "🌈"];

#[component]
fn App() -> Element {
    // 当前显示的消息
    let mut message = use_signal(|| "请选择一个按钮".to_string());

    // 当前选中的按钮索引 (None 表示没有选中)
    let mut selected_button = use_signal(|| None::<usize>);

    // 点击计数器
    let mut click_count = use_signal(|| 0u32);

    rsx! {
        // 引入外部 CSS 文件
        document::Link {
            rel: "stylesheet",
            href: asset!("./assets/style.css"),
        }

        div {
            class: "app-container",

            // 标题
            h2 {
                class: "app-title",
                "Emoji 按钮选择器"
            }

            // 按钮容器
            div {
                class: "button-container",
                for (i, emoji) in BUTTONS.iter().enumerate() {
                    button {
                        key: "{i}",
                        class: if selected_button() == Some(i) {
                            "emoji-button selected"
                        } else {
                            "emoji-button"
                        },
                        onclick: move |_| {
                            // 更新选中状态
                            selected_button.set(Some(i));

                            // 更新消息
                            message.set(format!("已选择: {}", emoji));

                            // 增加点击计数
                            click_count.set(click_count() + 1);
                        },
                        "{emoji}"
                    }
                }
            }

            // 消息显示区域
            p {
                class: "message-display",
                "{message()}"
            }

            // 状态信息区域
            div {
                class: "status-info",

                div {
                    class: "status-title",
                    "选择状态:"
                }

                div {
                    class: "current-selection",
                    if let Some(index) = selected_button() {
                        "当前选择: {BUTTONS[index]} (索引: {index})"
                    } else {
                        "暂无选择"
                    }
                }

                div {
                    class: "selection-count",
                    "总点击次数: {click_count()}"
                }

                // 清除选择按钮
                button {
                    class: "clear-button",
                    disabled: selected_button().is_none(),
                    onclick: move |_| {
                        selected_button.set(None);
                        message.set("已清除选择".to_string());
                    },
                    "清除选择"
                }
            }
        }
    }
}
