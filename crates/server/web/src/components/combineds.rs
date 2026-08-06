// 组合订阅页：组合列表（成员数 + 三种格式链接复制）+ 新建/编辑弹窗（名字 + 成员勾选）。
use crate::api::request;
use crate::components::confirm::{ConfirmDialog, ConfirmState};
use crate::components::copy_text;
use crate::components::icon::{Spinner, icon};
use crate::components::sources::{SourceDto, fetch_sources};
use crate::components::toast::{ToastKind, push_toast, schedule_timeout, use_toast};
use dioxus::prelude::*;
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Debug, Clone, Deserialize)]
pub struct CombinedDto {
    pub id: i64,
    pub name: String,
    // 后端返回的字段，作为 API 契约保留；UI 暂不展示。
    #[allow(dead_code)]
    pub created_at: String,
    pub source_ids: Vec<i64>,
}

pub async fn fetch_combineds(token: Option<&str>) -> Result<Vec<CombinedDto>, String> {
    let body = request("GET", "/admin/combineds", None, token).await?;
    serde_json::from_str(&body).map_err(|e| format!("解析失败: {}", e))
}

// 弹窗表单状态：None = 关闭；Some(edit_id) = 编辑既有组合（name 预填）；新建时 Some(-1)
#[derive(Debug, Clone, Default)]
struct FormState {
    open: bool,
    edit_id: Option<i64>,
    name: String,
    checked: HashSet<i64>,
}

#[component]
pub fn Combineds(token: Signal<Option<String>>) -> Element {
    let combineds = use_signal(Vec::<CombinedDto>::new);
    let sources = use_signal(Vec::<SourceDto>::new);
    let mut error = use_signal(String::new);
    let mut form = use_signal(FormState::default);
    let mut saving = use_signal(|| false);
    let mut confirm = use_signal(ConfirmState::default);
    let mut pending_id = use_signal(|| None::<i64>);
    // 复制反馈按 (组合名, 格式) 键控：复制某一格式只翻转该按钮
    let mut copied = use_signal(|| None::<(String, String)>);
    let toasts = use_toast();

    // 初次挂载加载组合与源列表
    use_future(move || {
        let token = token.read().clone();
        let mut combineds = combineds;
        let mut sources = sources;
        let mut error = error;
        async move {
            let t = token.as_deref();
            match fetch_combineds(t).await {
                Ok(list) => combineds.set(list),
                Err(e) => error.set(e),
            }
            match fetch_sources(t).await {
                Ok(list) => sources.set(list),
                Err(e) => error.set(e),
            }
        }
    });

    // 打开新建弹窗
    let open_create = move |_| {
        form.set(FormState {
            open: true,
            edit_id: None,
            name: String::new(),
            checked: HashSet::new(),
        });
    };

    // 打开编辑弹窗（预填名字与成员）
    let mut open_edit = move |id: i64| {
        let c = combineds.read().iter().find(|c| c.id == id).cloned();
        if let Some(c) = c {
            form.set(FormState {
                open: true,
                edit_id: Some(id),
                name: c.name,
                checked: c.source_ids.iter().copied().collect(),
            });
        }
    };

    // 勾选切换（HashSet 的 contains/remove/insert 均为 O(1)）
    let mut toggle_member = move |sid: i64| {
        let mut f = form.read().clone();
        if f.checked.contains(&sid) {
            f.checked.remove(&sid);
        } else {
            f.checked.insert(sid);
        }
        form.set(f);
    };

    // 保存（新建或编辑）
    let save = move |_| {
        let f = form.read().clone();
        if f.name.is_empty() {
            error.set("组合名称不能为空".into());
            return;
        }
        let token = token.read().clone();
        let body = serde_json::json!({
            "name": f.name,
            "source_ids": f.checked,
        })
        .to_string();
        let mut form = form.clone();
        let mut combineds = combineds.clone();
        let mut error = error.clone();
        let mut saving = saving.clone();
        let mut toasts = toasts.clone();
        saving.set(true);
        spawn(async move {
            let result = match f.edit_id {
                Some(id) => {
                    request(
                        "PUT",
                        &format!("/admin/combineds/{}", id),
                        Some(body),
                        token.as_deref(),
                    )
                    .await
                }
                None => request("POST", "/admin/combineds", Some(body), token.as_deref()).await,
            };
            match result {
                Ok(_) => match fetch_combineds(token.as_deref()).await {
                    Ok(list) => {
                        combineds.set(list);
                        form.set(FormState::default());
                        error.set(String::new());
                        push_toast(toasts, ToastKind::Success, "组合订阅已保存");
                    }
                    Err(e) => error.set(e),
                },
                Err(e) => error.set(format!("保存失败: {e}")),
            }
            saving.set(false);
        });
    };

    // 删除确认
    let mut ask_delete = move |id: i64| {
        let name = combineds
            .read()
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.name.clone())
            .unwrap_or_default();
        pending_id.set(Some(id));
        confirm.set(ConfirmState {
            open: true,
            title: "删除组合订阅".into(),
            message: format!("确定删除「{}」？此操作不可撤销。", name),
            confirm_text: "删除".into(),
            danger: true,
        });
    };
    let on_confirm_delete = use_callback(move |_: ()| {
        confirm.set(ConfirmState::default());
        if let Some(id) = pending_id() {
            let token = token.read().clone();
            let mut combineds = combineds.clone();
            let mut error = error.clone();
            let mut toasts = toasts.clone();
            spawn(async move {
                match request(
                    "DELETE",
                    &format!("/admin/combineds/{id}"),
                    None,
                    token.as_deref(),
                )
                .await
                {
                    Ok(_) => match fetch_combineds(token.as_deref()).await {
                        Ok(list) => {
                            combineds.set(list);
                            push_toast(toasts, ToastKind::Success, "已删除");
                        }
                        Err(e) => error.set(e),
                    },
                    Err(e) => push_toast(toasts, ToastKind::Error, format!("删除失败: {e}")),
                }
            });
        }
    });

    // 链接复制（(name, format) 门控反馈：复制某格式只翻转对应按钮）
    let copy_click = move |name: String, fmt: &str, link: String| {
        let key = (name, fmt.to_string());
        let mut copied = copied.clone();
        let toasts = toasts.clone();
        spawn(async move {
            match copy_text(link).await {
                Ok(()) => {
                    copied.set(Some(key.clone()));
                    push_toast(toasts, ToastKind::Success, "已复制到剪贴板");
                    let mut copied2 = copied.clone();
                    schedule_timeout(2000, move || {
                        if copied2.read().as_ref() == Some(&key) {
                            copied2.set(None);
                        }
                    });
                }
                Err(e) => push_toast(toasts, ToastKind::Error, format!("复制失败: {e}")),
            }
        });
    };

    // 弹窗内成员复选框行（预渲染）
    let member_rows: Vec<Element> = sources
        .read()
        .iter()
        .map(|s| {
            let sid = s.id;
            let name = s.name.clone();
            let kind = s.kind.clone();
            let checked = form.read().checked.contains(&sid);
            rsx! {
                label { class: "member-row",
                    input {
                        r#type: "checkbox",
                        checked,
                        onchange: move |_| toggle_member(sid),
                    }
                    span { "{name}" }
                    span { class: format!("badge {}", if kind == "single" { "info" } else { "off" }),
                        if kind == "single" { "单条" } else { "远程" }
                    }
                    if !s.enabled {
                        span { class: "badge off", "停用" }
                    }
                }
            }
        })
        .collect();

    // 组合行（预渲染）
    let rows: Vec<Element> = combineds
        .read()
        .iter()
        .map(|c| {
            let id = c.id;
            let name = c.name.clone();
            let count = c.source_ids.len();
            let base = web_sys::window()
                .and_then(|w| w.location().origin().ok())
                .unwrap_or_default();
            // 三种格式的链接按钮（预渲染，与行/成员行同模式）。
            // 订阅链接在 rsx 外拼装：rsx 内嵌 format! 的 {} 会被误判为插值（见 config.rs）。
            // move 闭包按值捕获 name，map 内逐次 clone，避免首轮即 move 掉外层 String。
            let link_buttons: Vec<Element> = ["clash", "v2ray", "singbox"]
                .into_iter()
                .map(|fmt| {
                    let link = format!("{}/subscribe/{}?format={}", base, name, fmt);
                    let name = name.clone();
                    let is_copied =
                        copied.read().as_ref() == Some(&(name.clone(), fmt.to_string()));
                    rsx! {
                        button {
                            class: format!("btn btn-ghost btn-sm{}", if is_copied { " checked" } else { "" }),
                            onclick: move |_| copy_click(name.clone(), fmt, link.clone()),
                            "{fmt}"
                        }
                    }
                })
                .collect();
            rsx! {
                div { class: "combined-row",
                    div { class: "combined-info",
                        span { class: "combined-name", "{name}" }
                        span { class: "badge on", "{count} 个成员" }
                    }
                    div { class: "combined-links",
                        {link_buttons.into_iter()}
                    }
                    div { class: "actions",
                        button { class: "btn btn-ghost btn-sm", onclick: move |_| open_edit(id), {icon("config", 13)} "编辑" }
                        button { class: "btn btn-danger btn-sm", onclick: move |_| ask_delete(id), {icon("trash", 13)} "删除" }
                    }
                }
            }
        })
        .collect();

    // 弹窗内成员勾选态由 member_rows 预渲染快照持有，这里只需 name 快照（受控回显）。
    let form_name = form.read().name.clone();
    rsx! {
        div { class: "page-head",
            h1 { class: "page-title", "组合订阅" }
            button { class: "btn btn-primary", onclick: open_create, {icon("plus", 14)} "新建组合" }
        }
        if !error.read().is_empty() {
            p { class: "error-text", "{error}" }
        }
        div { class: "card",
            if rows.is_empty() {
                div { class: "empty",
                    {icon("combineds", 36)}
                    span { class: "empty-title", "暂无组合订阅" }
                    span { class: "empty-hint", "新建组合并从订阅源中勾选成员，生成独立订阅链接" }
                }
            } else {
                {rows.into_iter()}
            }
        }
        // 新建/编辑弹窗
        if form.read().open {
            div { class: "modal-overlay", onclick: move |_| form.set(FormState::default()),
                div { class: "modal", onclick: move |e| e.stop_propagation(),
                    h3 { class: "modal-title", if form.read().edit_id.is_some() { "编辑组合" } else { "新建组合" } }
                    div { class: "field",
                        label { "组合名称（字母、数字、-、_）" }
                        input {
                            class: "mono",
                            placeholder: "例如：home",
                            value: form_name,
                            oninput: move |e| { let mut f = form.read().clone(); f.name = e.value(); form.set(f); },
                        }
                    }
                    p { class: "subtle", "选择包含的订阅源（可多选）" }
                    if member_rows.is_empty() {
                        div { class: "empty", span { class: "empty-hint", "暂无订阅源，请先到「订阅源」页添加" } }
                    } else {
                        {member_rows.into_iter()}
                    }
                    div { class: "modal-actions",
                        button { class: "btn btn-ghost", onclick: move |_| form.set(FormState::default()), "取消" }
                        button { class: "btn btn-primary", onclick: save, disabled: *saving.read(),
                            if *saving.read() { Spinner { size: 14 } } else { "保存" }
                        }
                    }
                }
            }
        }
        ConfirmDialog { state: confirm, on_confirm: on_confirm_delete }
    }
}
