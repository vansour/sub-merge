// crates/server/web/src/main.rs
mod api;
mod components;

use components::login::{clear_token, read_token, write_token, Login};
use dioxus::prelude::*;

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    let mut token = use_signal(|| read_token());
    rsx! {
        div {
            match token().as_deref() {
                Some(_) => rsx! {
                    MainShell { token }
                },
                None => rsx! {
                    Login {
                        on_login: move |t: String| {
                            write_token(&t);
                            token.set(Some(t));
                        },
                    }
                },
            }
        }
    }
}

// Task 3：主界面骨架。Tab 导航：订阅源 / 预览(Task4) / 配置(Task5)。
// 目前只实现 订阅源；预览/配置 两个 tab 用占位 div，Task4/5 替换为真实组件。
#[component]
fn MainShell(token: Signal<Option<String>>) -> Element {
    let mut tab = use_signal(|| 0usize);
    rsx! {
        div { class: "container",
            h1 { "sub-merge 管理" }
            nav { style: "margin-bottom: 16px",
                button { class: "secondary", onclick: move |_| tab.set(0), "订阅源" }
                button { class: "secondary", onclick: move |_| tab.set(1), "预览" }
                button { class: "secondary", onclick: move |_| tab.set(2), "配置" }
                button { class: "danger", onclick: move |_| {
                    clear_token();
                    token.set(None);
                }, "退出登录" }
            }
            match *tab.read() {
                0 => rsx! { components::sources::Sources { token } },
                // 预览/配置 占位，Task4/Task5 实现
                1 => rsx! { div { class: "card", p { "预览（Task 4 实现）" } } },
                _ => rsx! { div { class: "card", p { "配置（Task 5 实现）" } } },
            }
        }
    }
}
