use dioxus::prelude::*;

fn main() {
    dioxus::launch(App);
}

// 将按钮数据定义为静态常量
const BUTTONS: &[&str] = &["🔴", "🟠", "🟡", "🟢", "🔵", "🟣", "🟤", "⚪", "⚫", "🌈"];

#[component]
fn App() -> Element {
    let mut message = use_signal(|| "点击按钮".to_string());

    rsx! {
        document::Link {
            rel: "stylesheet",
            href: asset!("./assets/style.css"),
        }

        div {
            class: "app-container",

            h2 {
                class: "app-title",
                "Emoji按钮"
            }

            div {
                class: "button-container",
                for (i, emoji) in BUTTONS.iter().enumerate() {
                    button {
                        key: "{i}",
                        class: "emoji-button",
                        onclick: move |_| message.set(format!("点击了 {}", emoji)),
                        "{emoji}"
                    }
                }
            }

            p {
                class: "message-display",
                "{message()}"
            }
        }
    }
}
