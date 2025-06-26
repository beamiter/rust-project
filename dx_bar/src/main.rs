use dioxus::prelude::*;

fn main() {
    dioxus::launch(App);
}

// 静态数据：所有可用的 emoji 按钮
const BUTTONS: &[&str] = &["🔴", "🟠", "🟡", "🟢", "🔵", "🟣", "🟤", "⚪", "⚫", "🌈"];

#[component]
fn App() -> Element {
    // 响应式状态：存储当前显示的消息
    let mut message = use_signal(|| "点击按钮".to_string());

    rsx! {
        // 📁 引入外部 CSS 文件
        document::Link {
            rel: "stylesheet",                    // 链接类型：样式表
            href: asset!("./assets/style.css"),  // 文件路径（编译时处理）
        }

        // 🎨 主容器
        div {
            class: "app-container",  // 应用 CSS 类

            // 📰 标题
            h2 {
                class: "app-title",
                "Emoji按钮"
            }

            // 🔘 按钮容器
            div {
                class: "button-container",

                // 🔄 循环渲染按钮
                for (i, emoji) in BUTTONS.iter().enumerate() {
                    button {
                        key: "{i}",              // React-style key for optimization
                        class: "emoji-button",   // 应用 CSS 类
                        onclick: move |_| {      // 点击事件处理
                            message.set(format!("点击了 {}", emoji))
                        },
                        "{emoji}"               // 按钮内容
                    }
                }
            }

            // 💬 消息显示区域
            p {
                class: "message-display",
                "{message()}"  // 显示当前消息
            }
        }
    }
}
