// crates/server/web/src/components/sources.rs
// 订阅源 CRUD + 刷新：数据读 DataStore 缓存（MainShell 预载），CRUD 成功后 refresh 回写。
use crate::api::request;
use crate::components::confirm::{ConfirmDialog, ConfirmState};
use crate::components::icon::{Spinner, icon};
use crate::components::preview_section::PreviewSection;
use crate::components::toast::{ToastKind, push_toast, use_toast};
use crate::data::{DataStore, UnitKey};
use dioxus::prelude::*;
use submerge_web_core::fmt::kind_label;

// 订阅源页按 kind 参数化：本地（"single"）/ 远程（"remote"）两实例共用（导航结构 Task 4 建双入口）。
// 列表按 kind 过滤；添加表单类型固定（body 传 kind）；页面底部内嵌该 kind 的预览区。
#[component]
pub fn Sources(token: Signal<Option<String>>, kind: &'static str) -> Element {
    let data = use_context::<DataStore>();
    let mut error = use_signal(String::new);
    let mut new_url = use_signal(String::new);
    let mut new_name = use_signal(String::new);
    let adding = use_signal(|| false);
    let mut refreshing = use_signal(std::collections::HashSet::<i64>::new);
    let mut confirm = use_signal(ConfirmState::default);
    let mut pending_id = use_signal(|| None::<i64>);
    let toasts = use_toast();

    // 数据来自 DataStore 缓存（MainShell 预载）；CRUD 成功后 data.refresh 回写。
    // 列表按 kind 过滤：页内只展示本页类型的源（添加的源也固定为该 kind）。
    let sources_state = data.sources.read().clone();
    let source_list = sources_state
        .data
        .unwrap_or_default()
        .into_iter()
        .filter(|s| s.kind == kind)
        .collect::<Vec<_>>();

    let add = move |_| {
        let url = new_url.read().clone();
        let name = new_name.read().clone();
        if url.is_empty() || name.is_empty() {
            error.set("URL 和名称不能为空".into());
            return;
        }
        let token = token.read().clone();
        let body = serde_json::json!({ "url": url, "name": name, "kind": kind }).to_string();
        let mut new_url = new_url.clone();
        let mut new_name = new_name.clone();
        let mut error = error.clone();
        let mut adding = adding.clone();
        let toasts = toasts.clone();
        adding.set(true);
        spawn(async move {
            match request("POST", "/admin/sources", Some(body), token.as_deref()).await {
                Ok(_) => {
                    data.refresh(UnitKey::Sources);
                    new_url.set(String::new());
                    new_name.set(String::new());
                    error.set(String::new());
                    push_toast(toasts, ToastKind::Success, "订阅源已添加");
                }
                Err(e) => error.set(format!("添加失败: {e}")),
            }
            adding.set(false);
        });
    };

    let toggle = move |id: i64, enabled: bool| {
        let token = token.read().clone();
        let body = serde_json::json!({ "enabled": !enabled }).to_string();
        let toasts = toasts.clone();
        spawn(async move {
            match request(
                "PUT",
                &format!("/admin/sources/{id}"),
                Some(body),
                token.as_deref(),
            )
            .await
            {
                Ok(_) => {
                    data.refresh(UnitKey::Sources);
                    push_toast(
                        toasts,
                        ToastKind::Info,
                        if enabled { "已停用" } else { "已启用" },
                    );
                }
                Err(e) => push_toast(toasts, ToastKind::Error, format!("操作失败: {e}")),
            }
        });
    };

    let mut refresh = move |id: i64| {
        if refreshing.read().contains(&id) {
            return;
        }
        refreshing.write().insert(id);
        let token = token.read().clone();
        let mut refreshing = refreshing.clone();
        let toasts = toasts.clone();
        spawn(async move {
            match request(
                "POST",
                &format!("/admin/sources/{id}/refresh"),
                None,
                token.as_deref(),
            )
            .await
            {
                Ok(body) => match serde_json::from_str::<serde_json::Value>(&body) {
                    Ok(v) => {
                        let name = v.get("source").and_then(|s| s.as_str()).unwrap_or("该源");
                        match v.get("ok").and_then(|o| o.as_bool()) {
                            Some(true) => {
                                let n = v.get("node_count").and_then(|c| c.as_u64()).unwrap_or(0);
                                push_toast(
                                    toasts,
                                    ToastKind::Success,
                                    format!("{} 已刷新：{} 个节点", name, n),
                                );
                            }
                            _ => {
                                let reason = v
                                    .get("reason")
                                    .and_then(|r| r.as_str())
                                    .unwrap_or("未知错误");
                                push_toast(
                                    toasts,
                                    ToastKind::Error,
                                    format!("{} 刷新失败：{}", name, reason),
                                );
                            }
                        }
                    }
                    Err(e) => push_toast(toasts, ToastKind::Error, format!("刷新失败: {}", e)),
                },
                Err(e) => push_toast(toasts, ToastKind::Error, format!("刷新失败: {e}")),
            }
            refreshing.write().remove(&id);
        });
    };

    // 名字由行渲染处传入（ask_delete 仅捕获 Copy 信号，跨行内 move 闭包可复制）。
    let mut ask_delete = move |id: i64, name: String| {
        pending_id.set(Some(id));
        confirm.set(ConfirmState {
            open: true,
            title: "删除订阅源".into(),
            message: format!("确定删除「{}」？此操作不可撤销。", name),
            confirm_text: "删除".into(),
            danger: true,
        });
    };

    // 确认删除：关闭弹窗 → 执行 DELETE → refresh 回写缓存。
    let on_confirm_delete = use_callback(move |_: ()| {
        confirm.set(ConfirmState::default());
        if let Some(id) = pending_id() {
            let token = token.read().clone();
            let toasts = toasts.clone();
            spawn(async move {
                match request(
                    "DELETE",
                    &format!("/admin/sources/{id}"),
                    None,
                    token.as_deref(),
                )
                .await
                {
                    Ok(_) => {
                        data.refresh(UnitKey::Sources);
                        push_toast(toasts, ToastKind::Success, "已删除");
                    }
                    Err(e) => push_toast(toasts, ToastKind::Error, format!("删除失败: {e}")),
                }
            });
        }
    });

    // 行预渲染成 owned Element（沿用项目既有模式，避免 E0716 借用问题）。
    let rows: Vec<Element> = source_list
        .iter()
        .map(|s| {
            let id = s.id;
            let enabled = s.enabled;
            let name = s.name.clone();
            let kind = s.kind.clone();
            let url = s.url.clone();
            let busy = refreshing.read().contains(&id);
            rsx! {
                tr {
                    td { class: "cell-name", "{name}" }
                    td {
                        span { class: format!("badge {}", if kind == "single" { "info" } else { "off" }),
                            {kind_label(&kind)}
                        }
                    }
                    td { class: "cell-url", title: "{url}", "{url}" }
                    td {
                        span { class: format!("badge {}", if enabled { "on" } else { "off" }),
                            if enabled { "启用" } else { "停用" }
                        }
                    }
                    td {
                        div { class: "actions",
                            button { class: "btn btn-ghost btn-sm", onclick: move |_| toggle(id, enabled), disabled: busy,
                                {icon(if enabled { "x" } else { "check" }, 13)}
                                if enabled { "停用" } else { "启用" }
                            }
                            button { class: "btn btn-ghost btn-sm", onclick: move |_| refresh(id), disabled: busy,
                                if busy {
                                    Spinner { size: 12 }
                                } else {
                                    {icon("refresh", 13)}
                                }
                                "刷新"
                            }
                            button { class: "btn btn-danger btn-sm", onclick: move |_| ask_delete(id, name.clone()),
                                {icon("trash", 13)}
                                "删除"
                            }
                        }
                    }
                }
            }
        })
        .collect();

    // 表单错误优先展示；无表单错误时展示缓存加载错误。
    let page_error = if error.read().is_empty() {
        sources_state.error.clone()
    } else {
        error.read().clone()
    };
    rsx! {
        div { class: "page-head",
            h1 { class: "page-title", "订阅源" }
        }
        if !page_error.is_empty() {
            p { class: "error-text", "{page_error}" }
        }
        div { class: "card",
            h2 { class: "card-title", "添加订阅源" }
            div { class: "form-row",
                div { class: "field",
                    label { "订阅 URL" }
                    input {
                        class: "mono",
                        placeholder: if kind == "single" {
                            "ss://..., vmess://... 单条节点链接"
                        } else {
                            "https://example.com/sub"
                        },
                        value: new_url,
                        oninput: move |e| new_url.set(e.value()),
                    }
                }
                div { class: "field",
                    label { "名称" }
                    input {
                        placeholder: "例如：机场 A",
                        value: new_name,
                        oninput: move |e| new_name.set(e.value()),
                    }
                }
                button { class: "btn btn-primary", onclick: add, disabled: *adding.read(),
                    if *adding.read() {
                        Spinner { size: 14 }
                    } else {
                        {icon("plus", 14)}
                    }
                    "添加"
                }
            }
        }
        div { class: "card",
            h2 { class: "card-title", "订阅源列表" }
            span { class: "badge on", "{source_list.len()} 个源" }
            if rows.is_empty() {
                div { class: "empty",
                    {icon("sources", 36)}
                    span { class: "empty-title", "暂无订阅源" }
                    span { class: "empty-hint", "在上方表单填写名称与订阅 URL，点击「添加」开始" }
                }
            } else {
                div { class: "table-wrap",
                    table {
                        thead {
                            tr { th { "名称" } th { "类型" } th { "URL" } th { "状态" } th { "操作" } }
                        }
                        tbody {
                            {rows.into_iter()}
                        }
                    }
                }
            }
        }
        div { class: "card",
            h2 { class: "card-title", "预览" }
            PreviewSection { token, kind: Some(kind), combined: None }
        }
        ConfirmDialog { state: confirm, on_confirm: on_confirm_delete }
    }
}
