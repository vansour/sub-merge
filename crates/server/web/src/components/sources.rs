// crates/server/web/src/components/sources.rs
// 订阅源 CRUD：数据读 DataStore 缓存（MainShell 预载），CRUD 成功后 refresh 回写。
// 每行「预览/编辑/删除」三按钮：预览 → PreviewModal（source_id 单源预览）；
// 编辑 → EditSourceModal（本文件内私有全屏弹窗）；删除 → 确认弹窗。
use crate::api::request;
use crate::components::confirm::{ConfirmDialog, ConfirmState};
use crate::components::icon::{Spinner, icon};
use crate::components::preview_modal::PreviewModal;
use crate::components::toast::{ToastKind, push_toast, use_toast};
use crate::data::{DataStore, SourceDto, UnitKey};
use dioxus::prelude::*;
use submerge_web_core::fmt::kind_label;

// 订阅源页按 kind 参数化：本地（"single"）/ 远程（"remote"）两实例共用（导航结构 Task 4 建双入口）。
// 列表按 kind 过滤；添加表单类型固定（body 传 kind）。
#[component]
pub fn Sources(token: Signal<Option<String>>, kind: &'static str) -> Element {
    let data = use_context::<DataStore>();
    let mut error = use_signal(String::new);
    let mut new_url = use_signal(String::new);
    let mut new_name = use_signal(String::new);
    let adding = use_signal(|| false);
    let mut confirm = use_signal(ConfirmState::default);
    let mut pending_id = use_signal(|| None::<i64>);
    // 全屏弹窗开关：预览（源 id）/ 编辑（源数据）
    let mut previewing = use_signal(|| None::<i64>);
    let mut editing = use_signal(|| None::<SourceDto>);
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
    // 预览/编辑闭包只捕获 Copy 信号，行数据以 owned 参数行内传入（行内 clone 免跨闭包 move）。
    let rows: Vec<Element> = source_list
        .iter()
        .map(|s| {
            let id = s.id;
            let name = s.name.clone();
            let kind = s.kind.clone();
            let url = s.url.clone();
            let src = s.clone();
            rsx! {
                tr {
                    td { "data-label": "名称", class: "cell-name", "{name}" }
                    td { "data-label": "类型",
                        span { class: format!("badge {}", if kind == "single" { "info" } else { "off" }),
                            {kind_label(&kind)}
                        }
                    }
                    td { "data-label": "URL", class: "cell-url", title: "{url}", "{url}" }
                    td { "data-label": "操作", class: "cell-actions",
                        div { class: "actions",
                            button { class: "btn btn-ghost btn-sm", onclick: move |_| previewing.set(Some(id)),
                                {icon("preview", 13)}
                                "预览"
                            }
                            button { class: "btn btn-ghost btn-sm", onclick: move |_| editing.set(Some(src.clone())),
                                {icon("edit", 13)}
                                "编辑"
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
                div { class: "table-wrap table-wrap-sources",
                    table {
                        thead {
                            tr { th { "名称" } th { "类型" } th { "URL" } th { "操作" } }
                        }
                        tbody {
                            {rows.into_iter()}
                        }
                    }
                }
            }
        }
        if let Some(src) = editing.read().clone() {
            EditSourceModal {
                src,
                token,
                on_close: move |_| editing.set(None),
            }
        }
        if let Some(sid) = *previewing.read() {
            PreviewModal {
                source_id: Some(sid),
                combined: None,
                on_close: move |_| previewing.set(None),
            }
        }
        ConfirmDialog { state: confirm, on_confirm: on_confirm_delete }
    }
}

// 编辑订阅源弹窗（全屏）。URL/名称/类型（single/remote 下拉）；保存 → PUT /admin/sources/{id}
// → data.refresh(UnitKey::Sources) + toast + 关闭；取消 → 关闭。挂载锁背景滚动，卸载恢复。
#[component]
fn EditSourceModal(
    src: SourceDto,
    token: Signal<Option<String>>,
    on_close: EventHandler<()>,
) -> Element {
    let data = use_context::<DataStore>();
    let mut url = use_signal(|| src.url.clone());
    let mut name = use_signal(|| src.name.clone());
    let mut kind = use_signal(|| src.kind.clone());
    let saving = use_signal(|| false);
    let mut err = use_signal(String::new);
    let toasts = use_toast();

    // 挂载时锁背景滚动；卸载恢复（与 PreviewModal 同模式，不用页面级监听器）。
    use_effect(move || {
        if let Some(body) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.body())
        {
            let _ = body.style().set_property("overflow", "hidden");
        }
    });
    use_drop(|| {
        if let Some(body) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.body())
        {
            let _ = body.style().remove_property("overflow");
        }
    });

    let save = move |_| {
        let u = url.read().clone();
        let n = name.read().clone();
        if u.is_empty() || n.is_empty() {
            err.set("URL 和名称不能为空".into());
            return;
        }
        let token = token.read().clone();
        let body =
            serde_json::json!({ "url": u, "name": n, "kind": kind.read().clone() }).to_string();
        let id = src.id;
        let mut saving = saving.clone();
        let mut err = err.clone();
        let toasts = toasts.clone();
        let on_close = on_close.clone();
        saving.set(true);
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
                    push_toast(toasts, ToastKind::Success, "订阅源已更新");
                    on_close.call(());
                }
                Err(e) => err.set(format!("保存失败: {e}")),
            }
            saving.set(false);
        });
    };

    let title = format!("编辑「{}」", src.name);
    let form_err = err.read().clone();
    let close_head = on_close.clone();
    let close_foot = on_close.clone();

    rsx! {
        div { class: "fullscreen-modal",
            div { class: "fullscreen-modal-head",
                h2 { class: "fullscreen-modal-title", "{title}" }
                button { class: "btn btn-ghost btn-sm", onclick: move |_| close_head.call(()), {icon("x", 16)} }
            }
            div { class: "fullscreen-modal-body",
                div { class: "field",
                    label { "订阅 URL" }
                    input {
                        class: "mono",
                        value: url,
                        oninput: move |e| url.set(e.value()),
                    }
                }
                div { class: "field", style: "margin-top: 14px",
                    label { "名称" }
                    input {
                        value: name,
                        oninput: move |e| name.set(e.value()),
                    }
                }
                div { class: "field", style: "margin-top: 14px",
                    label { "类型" }
                    select {
                        value: kind.read().clone(),
                        onchange: move |e| kind.set(e.value()),
                        option { value: "single", "单条（single）" }
                        option { value: "remote", "订阅链接（remote）" }
                    }
                }
                if !form_err.is_empty() {
                    p { class: "error-text", "{form_err}" }
                }
            }
            div { class: "fullscreen-modal-foot",
                button { class: "btn btn-ghost", onclick: move |_| close_foot.call(()), "取消" }
                button { class: "btn btn-primary", onclick: save, disabled: *saving.read(),
                    if *saving.read() {
                        Spinner { size: 14 }
                    } else {
                        "保存"
                    }
                }
            }
        }
    }
}
