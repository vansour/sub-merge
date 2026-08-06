// crates/server/web/src/components/config.rs
// 配置页：订阅链接卡片（复制反馈）+ Token 管理（掩码显示 + 轮换确认）。
use crate::api::request;
use crate::components::confirm::{ConfirmDialog, ConfirmState};
use crate::components::copy_text;
use crate::components::icon::icon;
use crate::components::login::write_token;
use crate::components::toast::{ToastKind, push_toast, schedule_timeout, use_toast};
use dioxus::prelude::*;
use serde::Deserialize;
use std::rc::Rc;

#[derive(Debug, Clone, Deserialize)]
pub struct ConfigDto {
    pub admin_token: String,
    pub combined_name: String,
    pub subscribe_url: String,
}

#[component]
pub fn Config(token: Signal<Option<String>>) -> Element {
    let cfg = use_signal(|| None::<ConfigDto>);
    let mut error = use_signal(String::new);
    let mut new_name = use_signal(String::new);
    let mut copied = use_signal(|| None::<&'static str>);
    let mut show_admin = use_signal(|| false);
    let mut confirm = use_signal(ConfirmState::default);
    let mut pending_rotate = use_signal(|| false);
    let toasts = use_toast();

    // 初次挂载加载一次。
    use_future(move || {
        let token = token.read().clone();
        let mut cfg = cfg.clone();
        let mut new_name = new_name.clone();
        let mut error = error.clone();
        async move {
            match request("GET", "/admin/config", None, token.as_deref()).await {
                Ok(body) => match serde_json::from_str::<ConfigDto>(&body) {
                    Ok(c) => {
                        new_name.set(c.combined_name.clone());
                        cfg.set(Some(c));
                    }
                    Err(e) => error.set(format!("解析失败: {}", e)),
                },
                Err(e) => error.set(e.to_string()),
            }
        }
    });

    let rotate = move || {
        let current = token.read().clone();
        let body = serde_json::json!({ "rotate": "admin" }).to_string();
        let mut cfg = cfg.clone();
        let mut error = error.clone();
        let mut toasts = toasts.clone();
        // Signal 是 Copy：把 token signal 拷进闭包，rotating admin token 后同步会话。
        let mut token = token;
        spawn(async move {
            match request("PUT", "/admin/config", Some(body), current.as_deref()).await {
                Ok(b) => match serde_json::from_str::<ConfigDto>(&b) {
                    Ok(c) => {
                        // 服务端轮换 admin token 后，旧 token 立即失效（已实测 401）。
                        // 同步更新本地会话（localStorage + token signal）。
                        write_token(&c.admin_token);
                        token.set(Some(c.admin_token.clone()));
                        error.set(String::new());
                        cfg.set(Some(c));
                        push_toast(toasts, ToastKind::Success, "管理 token 已轮换");
                    }
                    Err(e) => error.set(format!("解析失败: {}", e)),
                },
                Err(e) => error.set(e.to_string()),
            }
        });
    };

    let mut ask_rotate = move || {
        pending_rotate.set(true);
        confirm.set(ConfirmState {
            open: true,
            title: "轮换管理 token".into(),
            message: "轮换后旧管理 token 立即失效，当前会话将自动更新为新 token。确定继续？".into(),
            confirm_text: "轮换".into(),
            danger: true,
        });
    };

    let on_confirm_rotate = use_callback(move |_: ()| {
        confirm.set(ConfirmState::default());
        if pending_rotate() {
            rotate();
        }
    });

    let mut save_name = move |_| {
        let name = new_name.read().clone();
        if name.is_empty() {
            error.set("名称不能为空".into());
            return;
        }
        let token = token.read().clone();
        let body = serde_json::json!({ "combined_name": name }).to_string();
        let mut cfg = cfg.clone();
        let mut error = error.clone();
        let mut toasts = toasts.clone();
        spawn(async move {
            match request("PUT", "/admin/config", Some(body), token.as_deref()).await {
                Ok(b) => match serde_json::from_str::<ConfigDto>(&b) {
                    Ok(c) => {
                        cfg.set(Some(c));
                        error.set(String::new());
                        push_toast(toasts, ToastKind::Success, "组合订阅名称已更新");
                    }
                    Err(e) => error.set(format!("解析失败: {}", e)),
                },
                Err(e) => error.set(format!("保存失败: {e}")),
            }
        });
    };

    let copy_click = move |label: &'static str, link: String| {
        let mut copied = copied.clone();
        let toasts = toasts.clone();
        spawn(async move {
            match copy_text(link).await {
                Ok(()) => {
                    copied.set(Some(label));
                    push_toast(toasts, ToastKind::Success, "已复制到剪贴板");
                    // 按 label 门控：2s 内复制了其他行时，不清掉那行的"已复制"反馈。
                    // 仅成功时调度 revert 定时器：复制失败则按钮不翻转为"已复制"。
                    schedule_timeout(2000, move || {
                        if *copied.read() == Some(label) {
                            copied.set(None);
                        }
                    });
                }
                Err(e) => push_toast(toasts, ToastKind::Error, format!("复制失败: {e}")),
            }
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
            [
                ("Clash", "clash"),
                ("V2Ray", "v2ray"),
                ("Sing-box", "singbox"),
            ]
            .into_iter()
            .map(|(label, fmt)| {
                let link = format!("{}{}?format={}", base_url, c.subscribe_url, fmt);
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
        if cfg_render.read().is_some() {
            div { class: "card",
                h2 { class: "card-title", "订阅链接" }
                p { class: "subtle", "将以下链接填入 Clash / V2Ray / Sing-box 客户端的订阅地址" }
                {link_rows.into_iter()}
            }
            div { class: "card",
                h2 { class: "card-title", "组合订阅" }
                p { class: "subtle", "组合订阅名决定输出链接路径：/subscribe/{{名称}}。仅限字母、数字、-、_。" }
                div { class: "form-row",
                    div { class: "field",
                        label { "组合订阅名称" }
                        input {
                            class: "mono",
                            value: new_name,
                            oninput: move |e| new_name.set(e.value()),
                        }
                    }
                    button { class: "btn btn-secondary", onclick: save_name, "保存名称" }
                }
            }
            div { class: "card",
                h2 { class: "card-title", "Token" }
                p { class: "subtle", "管理 token 轮换后，当前浏览器会话自动切换到新 token；其他设备需重新登录。" }
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
                    button { class: "btn btn-danger btn-sm", onclick: move |_| ask_rotate(), "轮换" }
                }
            }
        }
        if !error.read().is_empty() {
            p { class: "error-text", "{error}" }
        }
        ConfirmDialog { state: confirm, on_confirm: on_confirm_rotate }
    }
}
