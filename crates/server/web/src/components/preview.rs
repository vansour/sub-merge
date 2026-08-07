// crates/server/web/src/components/preview.rs
// 转换预览：节点表（协议彩色徽章）+ 源错误警告卡片。
// 页头下拉选择「全部源 / 各组合」（请求 /admin/preview?combined=<名称> 过滤）。
use crate::api::request;
use crate::data::fetch_combineds;
use submerge_web_core::dto::CombinedDto;
use crate::components::icon::{icon, Spinner};
use dioxus::prelude::*;
use submerge_web_core::dto::PreviewResp;
use submerge_web_core::fmt::proto_class;

#[component]
pub fn Preview(token: Signal<Option<String>>) -> Element {
    let data = use_signal(|| None::<PreviewResp>);
    let loading = use_signal(|| false);
    let error = use_signal(String::new);
    let combineds = use_signal(Vec::<CombinedDto>::new);
    // None = 全部源；Some(名称) = 按组合过滤。
    let mut selected = use_signal(|| None::<String>);

    // 抽取的预览加载逻辑：use_future 初始加载、刷新按钮、下拉切换三处共用。
    // use_callback 保持跨渲染稳定，闭包内只读 token/写 data/loading/error 信号。
    let load_preview = use_callback(move |selected: Option<String>| {
        let token = token.read().clone();
        let mut data = data.clone();
        let mut loading = loading.clone();
        let mut error = error.clone();
        spawn(async move {
            loading.set(true);
            error.set(String::new());
            let path = match &selected {
                Some(name) => format!("/admin/preview?combined={}", name),
                None => "/admin/preview".to_string(),
            };
            match request("GET", &path, None, token.as_deref()).await {
                Ok(body) => match serde_json::from_str::<PreviewResp>(&body) {
                    Ok(r) => data.set(Some(r)),
                    Err(e) => error.set(format!("解析失败: {}", e)),
                },
                Err(e) => error.set(e.to_string()),
            }
            loading.set(false);
        });
    });

    // 初次挂载：并行加载组合列表（下拉选项）与初始预览（全部源）。
    use_future(move || {
        load_preview(None);
        let token = token.read().clone();
        let mut combineds = combineds;
        let mut error = error;
        async move {
            match fetch_combineds(token.as_deref()).await {
                Ok(list) => combineds.set(list),
                Err(e) => error.set(e),
            }
        }
    });

    let reload = move |_| {
        load_preview(selected.read().clone());
    };

    let resp = data.read().clone();
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

    // 下拉选项（预渲染，与行/成员行同模式）。selected 属性与当前选择比对。
    let combined_options: Vec<Element> = combineds
        .read()
        .iter()
        .map(|c| {
            let name = c.name.clone();
            let is_selected = selected.read().as_deref() == Some(name.as_str());
            rsx! {
                option { value: name.clone(), selected: is_selected, "{name}" }
            }
        })
        .collect();

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
            button { class: "btn btn-secondary", onclick: reload, disabled: *loading.read(),
                if *loading.read() {
                    Spinner { size: 14 }
                } else {
                    {icon("refresh", 14)}
                }
                "刷新预览"
            }
        }
        if !error.read().is_empty() {
            p { class: "error-text", "{error}" }
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
