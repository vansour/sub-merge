// crates/server/web/src/components/login.rs
use crate::api::request;
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

    let submit = move |_| {
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
        div { class: "container",
            div { class: "card",
                h2 { "登录" }
                p { "请输入管理 token 进入管理界面" }
                input {
                    placeholder: "管理 token",
                    value: input,
                    oninput: move |e| input.set(e.value()),
                }
                if !error.read().is_empty() {
                    p { style: "color: #ff3b30", "{error}" }
                }
                button { onclick: submit, disabled: *loading.read(), "登录" }
            }
        }
    }
}
