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
        div {
            style: "padding: 20px; text-align: center; font-family: system-ui;",

            h2 { "Emoji按钮" }

            div {
                style: "display: flex; gap: 10px; justify-content: center; margin: 20px 0;",
                for (i, emoji) in BUTTONS.iter().enumerate() {
                    button {
                        key: "{i}",
                        style: "
                            font-size: 30px; 
                            padding: 10px; 
                            border: 1px solid #ccc; 
                            border-radius: 8px; 
                            background: white;
                            cursor: pointer;
                        ",
                        onclick: move |_| message.set(format!("点击了 {}", emoji)),
                        "{emoji}"
                    }
                }
            }

            p { "{message()}" }
        }
    }
}
