// crates/server/web/src/components/config.rs
// 配置页：Token 管理（掩码显示 + 轮换确认）。
// 订阅链接与组合名称已迁移至「组合订阅」页，本页只保留 Token 卡片。
use crate::api::request;
use crate::components::confirm::{ConfirmDialog, ConfirmState};
use crate::components::login::write_token;
use crate::components::toast::{ToastKind, push_toast, use_toast};
use dioxus::prelude::*;
use submerge_web_core::dto::ConfigDto;
use submerge_web_core::fmt::mask_token;

#[component]
pub fn Config(token: Signal<Option<String>>) -> Element {
    let cfg = use_signal(|| None::<ConfigDto>);
    let mut error = use_signal(String::new);
    let mut show_admin = use_signal(|| false);
    let mut confirm = use_signal(ConfirmState::default);
    let mut pending_rotate = use_signal(|| false);
    let toasts = use_toast();

    // 初次挂载加载一次。
    use_future(move || {
        let token = token.read().clone();
        let mut cfg = cfg.clone();
        let mut error = error.clone();
        async move {
            match request("GET", "/admin/config", None, token.as_deref()).await {
                Ok(body) => match serde_json::from_str::<ConfigDto>(&body) {
                    Ok(c) => cfg.set(Some(c)),
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

    // 管理 token 的展示值（掩码切换）在 rsx 外预计算：
    // rsx 文本插值 `{...}` 里不能内嵌字符串字面量——内层引号会被 rustc 词法解析截断
    // （实测报 unknown start of token）。
    let admin_token_show = cfg
        .read()
        .as_ref()
        .map(|c| {
            if *show_admin.read() {
                c.admin_token.clone()
            } else {
                mask_token(&c.admin_token).to_string()
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
