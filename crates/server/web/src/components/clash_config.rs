// crates/server/web/src/components/clash_config.rs
// Clash 默认配置页：YAML 模板编辑（头部字段/dns/分流 rules 等用户自定义段）。
// proxy-providers 与 proxy-groups 由系统自动追加，无需（也不应）在此编写。
// 数据读 DataStore 缓存（MainShell 预载）；保存走 PUT /admin/clash-config。
use crate::api::request;
use crate::components::icon::Spinner;
use crate::components::toast::{ToastKind, push_toast, use_toast};
use crate::data::{DataStore, UnitKey};
use dioxus::prelude::*;

#[component]
pub fn ClashConfig(token: Signal<Option<String>>) -> Element {
    let data = use_context::<DataStore>();
    let mut draft = use_signal(String::new);
    let mut inited = use_signal(|| false);
    let mut error = use_signal(String::new);
    let saving = use_signal(|| false);
    let toasts = use_toast();

    // 挂载时从缓存单元初始化草稿。use_effect 在 effect 内读到的信号变化时重跑
    // （含缓存刷新后回写），inited 守卫保证不覆盖用户已编辑内容。
    let state = data.clash_config.read().clone();
    use_effect(move || {
        if !inited() {
            if let Some(t) = data.clash_config.read().data.clone() {
                draft.set(t);
                inited.set(true);
            }
        }
    });

    let save = move |_| {
        let t = draft.read().clone();
        if t.trim().is_empty() {
            error.set("模板不能为空".into());
            return;
        }
        let current = token.read().clone();
        let body = serde_json::json!({ "template": t }).to_string();
        let mut error = error.clone();
        let mut saving = saving.clone();
        let toasts = toasts.clone();
        saving.set(true);
        spawn(async move {
            match request("PUT", "/admin/clash-config", Some(body), current.as_deref()).await {
                Ok(_) => {
                    data.refresh(UnitKey::ClashConfig);
                    error.set(String::new());
                    push_toast(toasts, ToastKind::Success, "Clash 配置已保存");
                }
                Err(e) => error.set(format!("保存失败: {e}")),
            }
            saving.set(false);
        });
    };

    // 保存/校验错误优先展示；无本地错误时展示缓存加载错误。
    let page_error = if error.read().is_empty() {
        state.error.clone()
    } else {
        error.read().clone()
    };

    rsx! {
        div { class: "page-head",
            h1 { class: "page-title", "Clash 配置" }
            button { class: "btn btn-primary", onclick: save, disabled: *saving.read(),
                if *saving.read() { Spinner { size: 14 } } else { "保存" }
            }
        }
        if !page_error.is_empty() {
            p { class: "error-text", "{page_error}" }
        }
        div { class: "card",
            p { class: "subtle", "在此编辑 Clash 输出的默认配置（头部字段、dns、分流 rules 等）。proxy-providers 与 proxy-groups 由系统自动追加，无需（也不应）在此编写。" }
            textarea {
                class: "clash-template",
                rows: "24",
                placeholder: "mixed-port: 7890\nallow-lan: false\nmode: rule\nlog-level: info\n\nrules:\n  - MATCH,🚀 节点选择",
                value: draft,
                oninput: move |e| draft.set(e.value()),
            }
        }
    }
}
