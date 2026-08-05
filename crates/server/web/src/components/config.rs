// crates/server/web/src/components/config.rs
// Task 5：配置页。拉取 /api/admin/config 展示订阅链接 + token，支持复制与轮换。
use crate::api::request;
use dioxus::prelude::*;
use serde::Deserialize;
use std::rc::Rc;
use wasm_bindgen_futures::JsFuture;

#[derive(Debug, Clone, Deserialize)]
pub struct ConfigDto {
    pub subscribe_token: String,
    pub admin_token: String,
    pub subscribe_url: String,
}

// web-sys 0.3.103 实测签名（与计划里的说明不同）：
//   Window::navigator() -> Navigator（直接返回，非 Option）
//   Navigator::clipboard() -> Clipboard（直接返回，非 Result）
//   Clipboard::write_text(&str) -> js_sys::Promise（用 JsFuture await）
//   Window::location() -> Location；Location::href() -> Result<String, JsValue>
fn copy_text(text: String) {
    if let Some(nav) = web_sys::window().map(|w| w.navigator()) {
        let clip = nav.clipboard();
        spawn(async move {
            let _ = JsFuture::from(clip.write_text(&text)).await;
        });
    }
}

#[component]
pub fn Config(token: Signal<Option<String>>) -> Element {
    let cfg = use_signal(|| None::<ConfigDto>);
    let error = use_signal(String::new);
    let mut copied = use_signal(String::new);

    // 初次挂载时加载一次。用 use_future（挂载时只跑一次），
    // 避免计划里的 spawn-on-render 模式在每次 render 时重复发起请求。
    use_future(move || {
        let token = token.read().clone();
        let mut cfg = cfg.clone();
        let mut error = error.clone();
        async move {
            match request("GET", "/api/admin/config", None, token.as_deref()).await {
                Ok(body) => match serde_json::from_str::<ConfigDto>(&body) {
                    Ok(c) => cfg.set(Some(c)),
                    Err(e) => error.set(format!("解析失败: {}", e)),
                },
                Err(e) => error.set(e),
            }
        }
    });

    let rotate = move |which: &'static str| {
        let token = token.read().clone();
        let body = serde_json::json!({ "rotate": which }).to_string();
        let mut cfg = cfg.clone();
        let mut error = error.clone();
        spawn(async move {
            match request("PUT", "/api/admin/config", Some(body), token.as_deref()).await {
                Ok(b) => match serde_json::from_str::<ConfigDto>(&b) {
                    Ok(c) => cfg.set(Some(c)),
                    Err(e) => error.set(format!("解析失败: {}", e)),
                },
                Err(e) => error.set(e),
            }
        });
    };

    // base_url 取当前页面地址。去掉末尾 '/'，避免拼出 `host//api/subscribe`。
    let base_url = web_sys::window()
        .and_then(|w| w.location().href().ok())
        .unwrap_or_default();
    let base_url = base_url.trim_end_matches('/').to_string();

    // 订阅链接在 rsx 外预计算：
    //  - rsx 内嵌的 `format!` 含 `{}` 占位符会被 rsx 解析器误判为插值；
    //  - `for` 循环体内也不能放 `let` 语句（dioxus rsx 解析器限制）。
    let links: Vec<(&str, String)> = cfg
        .read()
        .as_ref()
        .map(|c| {
            [("Clash", "clash"), ("V2Ray", "v2ray"), ("Sing-box", "singbox")]
                .into_iter()
                .map(|(label, fmt)| {
                    // subscribe_url 来自后端 DTO（"/api/subscribe"），作为 API 契约使用。
                    let link = format!("{}{}?token={}&format={}", base_url, c.subscribe_url, c.subscribe_token, fmt);
                    (label, link)
                })
                .collect()
        })
        .unwrap_or_default();

    // 复制按钮行预渲染成 owned Element，onclick 闭包里直接调 copy_text + 更新 copied，
    // 避免引用组件作用域里的嵌套闭包（事件处理器需 'static）。
    let link_rows: Vec<Element> = links
        .iter()
        .map(|(label, link)| {
            let label = *label;
            // 事件处理器是 FnMut，会多次调用；用 Rc 在闭包内 clone，避免 move 出 captured String。
            let link_for_copy = Rc::new(link.clone());
            rsx! {
                div {
                    strong { "{label} " }
                    code { "{link}" }
                    button {
                        class: "secondary",
                        onclick: move |_| {
                            copy_text(link_for_copy.as_ref().clone());
                            copied.set("已复制".into());
                        },
                        "复制"
                    }
                }
            }
        })
        .collect();

    rsx! {
        div { class: "card",
            h2 { "配置" }
            if let Some(c) = cfg.read().as_ref() {
                h3 { "订阅链接" }
                {link_rows.into_iter()}
                if !copied.read().is_empty() {
                    p { style: "color: #1a7f37", "{copied}" }
                }
                hr {}
                h3 { "Token" }
                p { "订阅 token: " code { "{c.subscribe_token}" } }
                button { class: "secondary", onclick: move |_| rotate("subscribe"), "轮换订阅 token" }
                p { "管理 token: " code { "{c.admin_token}" } }
                button { class: "danger", onclick: move |_| rotate("admin"), "轮换管理 token" }
            }
            if !error.read().is_empty() {
                p { style: "color: #ff3b30", "{error}" }
            }
        }
    }
}
