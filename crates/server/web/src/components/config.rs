// crates/server/web/src/components/config.rs
// 配置页：账号管理（显示用户名 + 修改密码）。
// 数据读 DataStore 缓存（MainShell 预载）；改密成功后后端使全部会话失效（含当前），
// 本地清除会话回登录页。
use crate::api::request;
use crate::components::icon::Spinner;
use crate::components::login::clear_token;
use crate::components::toast::{ToastKind, push_toast, use_toast};
use crate::data::{DataStore, UnitKey};
use dioxus::prelude::*;

#[component]
pub fn Config(token: Signal<Option<String>>) -> Element {
    let data = use_context::<DataStore>();
    let mut error = use_signal(String::new);

    // 状态
    let mut old_pass = use_signal(String::new);
    let mut new_pass = use_signal(String::new);
    let mut new_pass2 = use_signal(String::new);
    let mut changing = use_signal(|| false);

    // 数据来自 DataStore 缓存（MainShell 预载）。
    let config_state = data.config.read().clone();

    // 提交改密：成功后服务端全部会话失效（含当前），清除本地会话回登录页。
    let mut do_change = move || {
        let old = old_pass.read().clone();
        let new_p = new_pass.read().clone();
        let new2 = new_pass2.read().clone();
        if new_p.is_empty() || old.is_empty() {
            error.set("请填写完整".into());
            return;
        }
        if new_p != new2 {
            error.set("两次输入的新密码不一致".into());
            return;
        }
        let current = token.read().clone();
        let body = serde_json::json!({"change_password": {"old": old, "new": new_p}}).to_string();
        let mut token2 = token.clone();
        changing.set(true);
        spawn(async move {
            match request("PUT", "/admin/config", Some(body), current.as_deref()).await {
                Ok(_) => {
                    // 会话已失效（服务端已删全部会话），直接回登录页；toast 会被卸载不渲染，故不发
                    clear_token();
                    token2.set(None);
                }
                Err(e) => error.set(format!("修改失败: {e}")),
            }
            changing.set(false);
        });
    };

    // 用户名展示值在 rsx 外预计算。
    let username = config_state
        .data
        .as_ref()
        .map(|c| c.username.clone())
        .unwrap_or_default();

    // 订阅输出设置：v2ray 是否使用 base64（draft 从缓存初始化，保存走 PUT /admin/config）。
    let mut v2ray_b64 = use_signal(|| {
        config_state
            .data
            .as_ref()
            .map(|c| c.v2ray_base64)
            .unwrap_or(true)
    });
    let setting_saving = use_signal(|| false);
    let toasts = use_toast();
    let save_setting = move |_| {
        let v = *v2ray_b64.read();
        let current = token.read().clone();
        let body = serde_json::json!({ "v2ray_base64": v }).to_string();
        let mut error = error.clone();
        let mut saving = setting_saving.clone();
        let toasts = toasts.clone();
        saving.set(true);
        spawn(async move {
            match request("PUT", "/admin/config", Some(body), current.as_deref()).await {
                Ok(_) => {
                    data.refresh(UnitKey::Config);
                    error.set(String::new());
                    push_toast(toasts, ToastKind::Success, "订阅输出设置已保存");
                }
                Err(e) => error.set(format!("保存失败: {e}")),
            }
            saving.set(false);
        });
    };

    // 改密错误优先展示；无本地错误时展示缓存加载错误。
    let page_error = if error.read().is_empty() {
        config_state.error.clone()
    } else {
        error.read().clone()
    };

    rsx! {
        div { class: "page-head",
            h1 { class: "page-title", "配置" }
        }
        if config_state.data.is_some() {
            div { class: "card",
                h2 { class: "card-title", "账号" }
                p { class: "subtle", "修改密码后所有设备（含当前会话）将被强制重新登录。" }
                div { class: "token-row",
                    span { class: "token-label", "用户名" }
                    code { class: "token-value", "{username}" }
                }
                div { class: "field",
                    input {
                        type: "password",
                        placeholder: "当前密码",
                        value: old_pass,
                        oninput: move |e| old_pass.set(e.value()),
                    }
                }
                div { class: "field",
                    input {
                        type: "password",
                        placeholder: "新密码",
                        value: new_pass,
                        oninput: move |e| new_pass.set(e.value()),
                    }
                }
                div { class: "field",
                    input {
                        type: "password",
                        placeholder: "确认新密码",
                        value: new_pass2,
                        oninput: move |e| new_pass2.set(e.value()),
                    }
                }
                button { class: "btn btn-primary", onclick: move |_| do_change(), disabled: *changing.read(),
                    if *changing.read() {
                        Spinner { size: 14 }
                    } else {
                        "修改密码"
                    }
                }
            }
            div { class: "card",
                h2 { class: "card-title", "订阅输出" }
                p { class: "subtle", "v2ray 订阅输出是否使用 base64 编码；关闭后浏览器直接显示节点 URI 文本（客户端两种形态均可解析）。" }
                div { class: "field",
                    label { class: "switch-row",
                        input {
                            r#type: "checkbox",
                            checked: *v2ray_b64.read(),
                            oninput: move |e| v2ray_b64.set(e.value() == "true"),
                        }
                        span { "v2ray 输出使用 base64" }
                    }
                }
                button { class: "btn btn-primary", onclick: save_setting, disabled: *setting_saving.read(),
                    if *setting_saving.read() {
                        Spinner { size: 14 }
                    } else {
                        "保存设置"
                    }
                }
            }
        }
        if !page_error.is_empty() {
            p { class: "error-text", "{page_error}" }
        }
    }
}
