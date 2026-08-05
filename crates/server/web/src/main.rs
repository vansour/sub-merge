// crates/server/web/src/main.rs
mod api;
mod components;

use components::login::{read_token, write_token, Login};
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
                // Task 2 占位；Task 3 替换为 components::main_shell::MainShell
                Some(_) => rsx! {
                    div { class: "container",
                        h1 { "sub-merge 管理" }
                        p { "已登录（主界面 Task 3 实现）" }
                    }
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
