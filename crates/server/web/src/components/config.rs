// crates/server/web/src/components/config.rs
// 配置页：订阅链接卡片（复制反馈）+ Token 管理（掩码显示 + 轮换确认）。
use crate::api::request;
use crate::components::confirm::{ConfirmDialog, ConfirmState};
use crate::components::icon::icon;
use crate::components::login::write_token;
use crate::components::toast::{push_toast, schedule_timeout, use_toast, ToastKind};
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
    let mut copied = use_signal(|| None::<&'static str>);
    let mut show_admin = use_signal(|| false);
    let mut confirm = use_signal(ConfirmState::default);
    let mut pending_rotate = use_signal(|| None::<&'static str>);
    let toasts = use_toast();

    // 初次挂载加载一次。
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
        let current = token.read().clone();
        let body = serde_json::json!({ "rotate": which }).to_string();
        let mut cfg = cfg.clone();
        let mut error = error.clone();
        let mut toasts = toasts.clone();
        // Signal 是 Copy：把 token signal 拷进闭包，rotating admin token 后同步会话。
        let mut token = token;
        spawn(async move {
            match request("PUT", "/api/admin/config", Some(body), current.as_deref()).await {
                Ok(b) => match serde_json::from_str::<ConfigDto>(&b) {
                    Ok(c) => {
                        // 服务端轮换 admin token 后，旧 token 立即失效（已实测 401）。
                        // 同步更新本地会话（localStorage + token signal）。
                        if which == "admin" {
                            write_token(&c.admin_token);
                            token.set(Some(c.admin_token.clone()));
                        }
                        error.set(String::new());
                        cfg.set(Some(c));
                        push_toast(toasts, ToastKind::Success, format!("{} token 已轮换", if which == "admin" { "管理" } else { "订阅" }));
                    }
                    Err(e) => error.set(format!("解析失败: {}", e)),
                },
                Err(e) => error.set(e),
            }
        });
    };

    let mut ask_rotate = move |which: &'static str| {
        pending_rotate.set(Some(which));
        let admin = which == "admin";
        confirm.set(ConfirmState {
            open: true,
            title: format!("轮换{} token", if admin { "管理" } else { "订阅" }),
            message: if admin {
                "轮换后旧管理 token 立即失效，当前会话将自动更新为新 token。确定继续？".into()
            } else {
                "轮换后旧订阅 token 立即失效，所有已复制的订阅链接需要重新复制。确定继续？".into()
            },
            confirm_text: "轮换".into(),
            danger: admin,
        });
    };

    let on_confirm_rotate = use_callback(move |_: ()| {
        confirm.set(ConfirmState::default());
        if let Some(which) = pending_rotate() {
            rotate(which);
        }
    });

    let mut copy_click = move |label: &'static str, link: String| {
        copy_text(link);
        copied.set(Some(label));
        let mut copied = copied.clone();
        schedule_timeout(2000, move || {
            copied.set(None);
        });
    };

    // base_url 取当前页面 origin（协议 + 主机 + 端口，不含路径）。
    // 不能用 href()：页面 URL 带路径（如 /index.html）时会把路径拼进订阅链接。
    let base_url = web_sys::window()
        .and_then(|w| w.location().origin().ok())
        .unwrap_or_default();

    // 订阅链接在 rsx 外预计算（rsx 内嵌 format! 的 {} 会被误判为插值）。
    let links: Vec<(&'static str, String)> = cfg
        .read()
        .as_ref()
        .map(|c| {
            [("Clash", "clash"), ("V2Ray", "v2ray"), ("Sing-box", "singbox")]
                .into_iter()
                .map(|(label, fmt)| {
                    let link = format!("{}{}?token={}&format={}", base_url, c.subscribe_url, c.subscribe_token, fmt);
                    (label, link)
                })
                .collect()
        })
        .unwrap_or_default();

    let link_rows: Vec<Element> = links
        .iter()
        .map(|(label, link)| {
            let label = *label;
            // 事件处理器是 FnMut，会多次调用；用 Rc 在闭包内 clone，避免 move 出 captured String。
            let link_for_copy = Rc::new(link.clone());
            let is_copied = *copied.read() == Some(label);
            rsx! {
                div { class: "link-row",
                    span { class: "link-label", "{label}" }
                    code { class: "link-url", "{link}" }
                    button {
                        class: format!("btn btn-ghost btn-sm{}", if is_copied { " checked" } else { "" }),
                        onclick: move |_| {
                            copy_click(label, link_for_copy.as_ref().clone());
                        },
                        {icon("copy", 13)}
                        if is_copied { "已复制" } else { "复制" }
                    }
                }
            }
        })
        .collect();

    // 管理 token 的展示值（掩码切换）在 rsx 外预计算：
    // rsx 文本插值 `{...}` 里不能内嵌字符串字面量——内层引号会被 rustc 词法解析截断
    // （实测报 unknown start of token），与上方订阅链接 format! 同理。
    let admin_token_show = cfg
        .read()
        .as_ref()
        .map(|c| {
            if *show_admin.read() {
                c.admin_token.clone()
            } else {
                "••••••••".to_string()
            }
        })
        .unwrap_or_default();

    let mut cfg_render = cfg.clone();
    let mut show_admin_render = show_admin.clone();
    rsx! {
        div { class: "page-head",
            h1 { class: "page-title", "配置" }
        }
        if let Some(c) = cfg_render.read().as_ref() {
            div { class: "card",
                h2 { class: "card-title", "订阅链接" }
                p { class: "subtle", "将以下链接填入 Clash / V2Ray / Sing-box 客户端的订阅地址" }
                {link_rows.into_iter()}
            }
            div { class: "card",
                h2 { class: "card-title", "Token" }
                p { class: "subtle", "管理 token 轮换后，当前浏览器会话自动切换到新 token；其他设备需重新登录。" }
                div { class: "token-row",
                    span { class: "token-label", "订阅 token" }
                    code { class: "token-value", "{c.subscribe_token}" }
                    button { class: "btn btn-secondary btn-sm", onclick: move |_| ask_rotate("subscribe"), "轮换" }
                }
                div { class: "token-row",
                    span { class: "token-label", "管理 token" }
                    code { class: "token-value",
                        "{admin_token_show}"
                    }
                    button { class: "btn btn-ghost btn-sm",
                        onclick: move |_| {
                            let v = *show_admin_render.read();
                            show_admin_render.set(!v);
                        },
                        if *show_admin_render.read() { "隐藏" } else { "显示" }
                    }
                    button { class: "btn btn-danger btn-sm", onclick: move |_| ask_rotate("admin"), "轮换" }
                }
            }
        }
        if !error.read().is_empty() {
            p { class: "error-text", "{error}" }
        }
        ConfirmDialog { state: confirm, on_confirm: on_confirm_rotate }
    }
}
