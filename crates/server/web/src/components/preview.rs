// crates/server/web/src/components/preview.rs
// 转换预览：节点表（协议彩色徽章）+ 源错误警告卡片。
// 页头下拉选择「全部源 / 各组合」。全部源视图读 preview 缓存单元（MainShell 预载）；
// 按组合过滤的视图为页面本地状态（请求 /admin/preview?combined=<名称>）。
use crate::api::request;
use crate::components::icon::{Spinner, icon};
use crate::data::{CacheStatus, DataStore, UnitKey};
use dioxus::prelude::*;
use submerge_web_core::dto::PreviewResp;
use submerge_web_core::fmt::proto_class;

#[component]
pub fn Preview(token: Signal<Option<String>>) -> Element {
    let data = use_context::<DataStore>();
    // 全部源视图来自 preview 缓存单元；按组合过滤的视图为页面本地状态。
    let local_data = use_signal(|| None::<PreviewResp>);
    let local_loading = use_signal(|| false);
    let local_error = use_signal(String::new);
    let mut selected = use_signal(|| None::<String>);

    // 下拉切换/刷新共用：全部源 → refresh 缓存单元；组合过滤 → 本地请求。
    let load_preview = use_callback(move |selected: Option<String>| {
        if selected.is_none() {
            data.refresh(UnitKey::Preview);
            return;
        }
        let token = token.read().clone();
        let mut local_data = local_data.clone();
        let mut local_loading = local_loading.clone();
        let mut local_error = local_error.clone();
        let name = selected.unwrap();
        spawn(async move {
            local_loading.set(true);
            local_error.set(String::new());
            let path = format!("/admin/preview?combined={name}");
            match request("GET", &path, None, token.as_deref()).await {
                Ok(body) => match serde_json::from_str::<PreviewResp>(&body) {
                    Ok(r) => local_data.set(Some(r)),
                    Err(e) => local_error.set(format!("解析失败: {}", e)),
                },
                Err(e) => local_error.set(e.to_string()),
            }
            local_loading.set(false);
        });
    });

    let reload = move |_| {
        load_preview(selected.read().clone());
    };

    // 数据快照与派生：全部源读缓存单元，组合过滤读本地状态。
    let preview_state = data.preview.read().clone();
    let combineds_state = data.combineds.read().clone();
    let resp = if selected.read().is_some() {
        local_data.read().clone()
    } else {
        preview_state.data.clone()
    };
    let rows: Vec<Element> = resp
        .as_ref()
        .map(|r| {
            r.nodes
                .iter()
                .map(|n| {
                    let name = n.name.clone();
                    let protocol = n.protocol.clone();
                    let server = n.server.clone();
                    let port = n.port;
                    rsx! {
                        tr {
                            td { class: "cell-name", "{name}" }
                            td { span { class: format!("proto {}", proto_class(&protocol)), "{protocol}" } }
                            td { class: "cell-url", "{server}" }
                            td { "{port}" }
                        }
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let error_rows: Vec<Element> = resp
        .as_ref()
        .map(|r| {
            r.errors
                .iter()
                .map(|e| {
                    let e = e.clone();
                    rsx! {
                        div { class: "error-line", {icon("alert", 14)} span { "{e}" } }
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    // 下拉选项来自 combineds 缓存单元（预渲染，与行/成员行同模式）。
    // selected 属性与当前选择比对；as_ref 借用，闭包不捕获 owned Vec。
    let combined_options: Vec<Element> = combineds_state
        .data
        .as_ref()
        .map(|list| {
            list.iter()
                .map(|c| {
                    let name = c.name.clone();
                    let is_selected = selected.read().as_deref() == Some(name.as_str());
                    rsx! {
                        option { value: name.clone(), selected: is_selected, "{name}" }
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    // 刷新/错误/loading 按选择派生：组合过滤 → 本地状态；全部源 → 缓存单元。
    let busy = if selected.read().is_some() {
        *local_loading.read()
    } else {
        preview_state.status == CacheStatus::Loading
    };
    let page_error = if selected.read().is_some() {
        local_error.read().clone()
    } else {
        preview_state.error.clone()
    };

    rsx! {
        div { class: "page-head",
            h1 { class: "page-title", "转换预览" }
            select {
                class: "preview-filter",
                value: selected.read().clone().unwrap_or_default(),
                onchange: move |e| {
                    let v = e.value();
                    let v = if v.is_empty() { None } else { Some(v) };
                    selected.set(v.clone());
                    load_preview(v);
                },
                option { value: "", "全部源" }
                {combined_options.into_iter()}
            }
            if let Some(r) = resp.as_ref() {
                span { class: "badge on", "共 {r.total} 个节点" }
            }
            button { class: "btn btn-secondary", onclick: reload, disabled: busy,
                if busy {
                    Spinner { size: 14 }
                } else {
                    {icon("refresh", 14)}
                }
                "刷新预览"
            }
        }
        if !page_error.is_empty() {
            p { class: "error-text", "{page_error}" }
        }
        if let Some(r) = resp.as_ref() {
            if r.nodes.is_empty() {
                div { class: "empty",
                    {icon("preview", 36)}
                    span { class: "empty-title", "暂无节点" }
                    span { class: "empty-hint", "检查订阅源是否已启用、刷新后重试" }
                }
            } else {
                div { class: "table-wrap",
                    table {
                        thead {
                            tr { th { "名称" } th { "协议" } th { "服务器" } th { "端口" } }
                        }
                        tbody {
                            {rows.into_iter()}
                        }
                    }
                }
            }
            if !r.errors.is_empty() {
                h2 { class: "card-title", style: "margin-top: 20px", "源错误" }
                div { class: "warning-box", {error_rows.into_iter()} }
            }
        }
    }
}
