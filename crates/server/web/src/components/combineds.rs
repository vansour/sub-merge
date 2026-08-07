// 组合订阅页：组合列表（成员数 + 三种格式链接复制）+ 新建/编辑弹窗（名字 + 成员勾选）。
// 数据读 DataStore 缓存（MainShell 预载）；保存/删除成功后 refresh 回写。
use crate::api::request;
use crate::components::confirm::{ConfirmDialog, ConfirmState};
use crate::components::copy_text;
use crate::components::icon::{Spinner, icon};
use crate::components::preview_section::PreviewSection;
use crate::components::toast::{ToastKind, push_toast, schedule_timeout, use_toast};
use crate::data::{DataStore, UnitKey};
use dioxus::prelude::*;
use std::collections::HashSet;
use submerge_web_core::fmt::{kind_label, subscribe_path};

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
    let data = use_context::<DataStore>();
    let mut error = use_signal(String::new);
    let mut form = use_signal(FormState::default);
    let saving = use_signal(|| false);
    let mut confirm = use_signal(ConfirmState::default);
    let mut pending_id = use_signal(|| None::<i64>);
    // 复制反馈按 (组合名, 格式) 键控：复制某一格式只翻转该按钮
    let copied = use_signal(|| None::<(String, String)>);
    // 组合预览区下拉选中值：onchange 更新 → PreviewSection 的 combined prop 变化触发重拉
    let mut preview_combined = use_signal(|| None::<String>);
    let toasts = use_toast();

    // 数据来自 DataStore 缓存（MainShell 预载）；保存/删除成功后 data.refresh 回写。
    let combineds_state = data.combineds.read().clone();
    let combined_list = combineds_state.data.unwrap_or_default();
    let sources_state = data.sources.read().clone();
    let source_list = sources_state.data.unwrap_or_default();

    // 打开新建弹窗
    let open_create = move |_| {
        form.set(FormState {
            open: true,
            edit_id: None,
            name: String::new(),
            checked: HashSet::new(),
        });
    };

    // 打开编辑弹窗（名字与成员由行渲染处传入——闭包不捕获 owned Vec，跨行内 move 可复制）
    let mut open_edit = move |id: i64, name: String, source_ids: Vec<i64>| {
        form.set(FormState {
            open: true,
            edit_id: Some(id),
            name,
            checked: source_ids.into_iter().collect(),
        });
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
        let mut error = error.clone();
        let mut saving = saving.clone();
        let toasts = toasts.clone();
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
                Ok(_) => {
                    data.refresh(UnitKey::Combineds);
                    form.set(FormState::default());
                    error.set(String::new());
                    push_toast(toasts, ToastKind::Success, "组合订阅已保存");
                }
                Err(e) => error.set(format!("保存失败: {e}")),
            }
            saving.set(false);
        });
    };

    // 删除确认（名字由行渲染处传入，与 sources.rs ask_delete 同模式）
    let mut ask_delete = move |id: i64, name: String| {
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
            let toasts = toasts.clone();
            spawn(async move {
                match request(
                    "DELETE",
                    &format!("/admin/combineds/{id}"),
                    None,
                    token.as_deref(),
                )
                .await
                {
                    Ok(_) => {
                        data.refresh(UnitKey::Combineds);
                        push_toast(toasts, ToastKind::Success, "已删除");
                    }
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
    let member_rows: Vec<Element> = source_list
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
                        {kind_label(&kind)}
                    }
                    if !s.enabled {
                        span { class: "badge off", "停用" }
                    }
                }
            }
        })
        .collect();

    // 组合行（预渲染）
    let rows: Vec<Element> = combined_list
        .iter()
        .map(|c| {
            let id = c.id;
            let name = c.name.clone();
            let count = c.source_ids.len();
            let source_ids = c.source_ids.clone();
            let base = web_sys::window()
                .and_then(|w| w.location().origin().ok())
                .unwrap_or_default();
            // 三种格式的链接按钮（预渲染，与行/成员行同模式）。
            // 订阅链接在 rsx 外拼装：rsx 内嵌 format! 的 {} 会被误判为插值（见 config.rs）。
            // move 闭包按值捕获 name，map 内逐次 clone，避免首轮即 move 掉外层 String。
            let link_buttons: Vec<Element> = ["clash", "v2ray", "singbox"]
                .into_iter()
                .map(|fmt| {
                    let link = format!("{}{}", base, subscribe_path(&name, fmt));
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
            // 编辑/删除闭包只捕获 Copy 信号，行数据以 owned 参数行内传入（行内 clone 免跨闭包 move）。
            let edit_name = name.clone();
            let del_name = name.clone();
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
                        // 事件处理器是 FnMut（可多次调用），非 Copy 捕获不能在体内 move 出——
                        // 行内 clone 后传参，与 ask_delete 同模式。
                        button { class: "btn btn-ghost btn-sm", onclick: move |_| open_edit(id, edit_name.clone(), source_ids.clone()), {icon("config", 13)} "编辑" }
                        button { class: "btn btn-danger btn-sm", onclick: move |_| ask_delete(id, del_name.clone()), {icon("trash", 13)} "删除" }
                    }
                }
            }
        })
        .collect();

    // 组合预览下拉选项（预渲染，同 preview.rs 模式）。下拉受控（value 绑定 preview_combined），
    // 组合改名/删除后不会显示漂移；空选项（value=""）对应「选择组合订阅」= 全部源预览。
    let combined_options: Vec<Element> = combined_list
        .iter()
        .map(|c| {
            let name = c.name.clone();
            rsx! {
                option { value: name.clone(), "{name}" }
            }
        })
        .collect();

    // 弹窗内成员勾选态由 member_rows 预渲染快照持有，这里只需 name 快照（受控回显）。
    let form_name = form.read().name.clone();
    // 表单错误优先展示；无表单错误时拼接缓存单元错误（combineds 主单元 + sources 次级单元，
    // 次级单元失败也要可见——否则成员勾选列表消失得无声无息，与 overview 的 join 模式一致）。
    let page_error = if !error.read().is_empty() {
        error.read().clone()
    } else {
        [combineds_state.error.clone(), sources_state.error.clone()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("; ")
    };
    rsx! {
        div { class: "page-head",
            h1 { class: "page-title", "组合订阅" }
            button { class: "btn btn-primary", onclick: open_create, {icon("plus", 14)} "新建组合" }
        }
        if !page_error.is_empty() {
            p { class: "error-text", "{page_error}" }
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
        div { class: "card",
            h2 { class: "card-title", "预览" }
            div {
                select {
                    class: "preview-filter",
                    value: preview_combined.read().clone().unwrap_or_default(),
                    onchange: move |e| {
                        let v = e.value();
                        let v = if v.is_empty() { None } else { Some(v) };
                        preview_combined.set(v);
                    },
                    option { value: "", "选择组合订阅" }
                    {combined_options.into_iter()}
                }
            }
            PreviewSection { token, kind: None, combined: preview_combined.read().clone() }
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
