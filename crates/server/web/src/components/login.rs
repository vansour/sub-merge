// crates/server/web/src/components/login.rs
use crate::api::request;
use crate::components::icon::{icon, Spinner};
use dioxus::prelude::*;

pub fn read_token() -> Option<String> {
    let w = web_sys::window()?;
    let s = w.local_storage().ok().flatten()?;
    s.get_item("submerge_admin_token").ok().flatten()
}

pub fn write_token(t: &str) {
    if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = s.set_item("submerge_admin_token", t);
    }
}

pub fn clear_token() {
    if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = s.remove_item("submerge_admin_token");
    }
}

#[component]
pub fn Login(on_login: EventHandler<String>) -> Element {
    let mut input = use_signal(String::new);
    let mut error = use_signal(String::new);
    let mut loading = use_signal(|| false);

    // 无参闭包：onclick（MouseEvent）与 onkeydown（KeyboardEvent）两种入口共享同一流程。
    // 不能直接把一个闭包同时传给两种事件处理器——Rust 闭包参数类型固定，两种事件类型不兼容。
    let mut do_submit = move || {
        if input.read().is_empty() {
            return;
        }
        let token = input.read().clone();
        loading.set(true);
        spawn(async move {
            // 用 GET /api/admin/config 验证 token 有效性
            match request("GET", "/api/admin/config", None, Some(&token)).await {
                Ok(_) => on_login.call(token),
                Err(e) => error.set(format!("登录失败: {}", e)),
            }
            loading.set(false);
        });
    };

    rsx! {
        div { class: "login-wrap",
            div { class: "login-card",
                div { class: "login-logo", {icon("logo", 40)} }
                div { class: "login-title", "sub-merge" }
                p { class: "login-sub", "订阅聚合与转换管理" }
                div { class: "field",
                    input {
                        type: "password",
                        placeholder: "管理 token",
                        value: input,
                        oninput: move |e| input.set(e.value()),
                        onkeydown: move |e| {
                            if e.key() == Key::Enter {
                                do_submit();
                            }
                        },
                    }
                }
                if !error.read().is_empty() {
                    p { class: "error-text", "{error}" }
                }
                button { class: "btn btn-primary", onclick: move |_| do_submit(), disabled: *loading.read(),
                    if *loading.read() {
                        Spinner { size: 14 }
                    } else {
                        "登录"
                    }
                }
            }
        }
    }
}
