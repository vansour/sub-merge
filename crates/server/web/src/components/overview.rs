// crates/server/web/src/components/overview.rs
// 概览页：4 张统计卡片（源总数/启用中/节点总数/失败源数）+ 订阅源摘要 + 最近错误。
// 数据来自现有两个接口（sources + preview），纯客户端聚合。
use crate::api::request;
use crate::components::icon::{icon, Spinner};
use crate::components::sources::{fetch_sources, SourceDto};
use dioxus::prelude::*;
use serde::Deserialize;

// 只取需要的字段；serde 默认忽略未知字段（nodes 等）。
#[derive(Debug, Clone, Deserialize)]
struct PreviewSummary {
    total: usize,
    errors: Vec<String>,
}

async fn fetch_preview(token: Option<&str>) -> Result<PreviewSummary, String> {
    let body = request("GET", "/api/admin/preview", None, token).await?;
    serde_json::from_str(&body).map_err(|e| format!("解析失败: {}", e))
}

#[component]
pub fn Overview(token: Signal<Option<String>>, on_goto: EventHandler<usize>) -> Element {
    let sources = use_signal(Vec::<SourceDto>::new);
    let stats = use_signal(|| None::<PreviewSummary>);
    let error = use_signal(String::new);
    let loading = use_signal(|| false);

    // 初次挂载加载一次。
    use_future(move || {
        let token = token.read().clone();
        let mut sources = sources;
        let mut stats = stats;
        let mut error = error;
        let mut loading = loading;
        async move {
            loading.set(true);
            error.set(String::new());
            match fetch_sources(token.as_deref()).await {
                Ok(list) => sources.set(list),
                Err(e) => error.set(e),
            }
            match fetch_preview(token.as_deref()).await {
                Ok(s) => stats.set(Some(s)),
                Err(e) => error.set(e),
            }
            loading.set(false);
        }
    });

    let reload = move |_| {
        let token = token.read().clone();
        let mut sources = sources.clone();
        let mut stats = stats.clone();
        let mut error = error.clone();
        let mut loading = loading.clone();
        spawn(async move {
            loading.set(true);
            error.set(String::new());
            match fetch_sources(token.as_deref()).await {
                Ok(list) => sources.set(list),
                Err(e) => error.set(e),
            }
            match fetch_preview(token.as_deref()).await {
                Ok(s) => stats.set(Some(s)),
                Err(e) => error.set(e),
            }
            loading.set(false);
        });
    };

    // 统计值在 rsx 外预计算（避免借用冲突）。
    let source_total = sources.read().len();
    let enabled_count = sources.read().iter().filter(|s| s.enabled).count();
    let (node_total, failed_count) = stats
        .read()
        .as_ref()
        .map(|s| (s.total, s.errors.len()))
        .unwrap_or((0, 0));

    let source_rows: Vec<Element> = sources
        .read()
        .iter()
        .map(|s| {
            let enabled = s.enabled;
            let name = s.name.clone();
            rsx! {
                div { class: "summary-row",
                    span { "{name}" }
                    span { class: format!("badge {}", if enabled { "on" } else { "off" }),
                        if enabled { "启用" } else { "停用" }
                    }
                }
            }
        })
        .collect();

    let error_rows: Vec<Element> = stats
        .read()
        .as_ref()
        .map(|s| {
            s.errors
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

    let on_goto = on_goto.clone();
    rsx! {
        div { class: "page-head",
            h1 { class: "page-title", "概览" }
            button { class: "btn btn-secondary", onclick: reload, disabled: *loading.read(),
                if *loading.read() {
                    Spinner { size: 14 }
                } else {
                    {icon("refresh", 14)}
                }
                "刷新"
            }
        }
        if !error.read().is_empty() {
            p { class: "error-text", "{error}" }
        }
        div { class: "stats-grid",
            StatCard { icon_name: "sources", value: source_total.to_string(), label: "订阅源总数", danger: false }
            StatCard { icon_name: "check", value: enabled_count.to_string(), label: "启用中", danger: false }
            StatCard { icon_name: "preview", value: node_total.to_string(), label: "节点总数", danger: false }
            StatCard { icon_name: "alert", value: failed_count.to_string(), label: "失败源数", danger: failed_count > 0 }
        }
        div { class: "grid-2",
            div { class: "card",
                h2 { class: "card-title", "订阅源" }
                if source_rows.is_empty() {
                    div { class: "empty",
                        {icon("sources", 36)}
                        span { class: "empty-title", "暂无订阅源" }
                        span { class: "empty-hint", "前往「订阅源」页面添加第一个源" }
                    }
                } else {
                    {source_rows.into_iter()}
                    div { class: "card-foot",
                        button { class: "btn btn-ghost btn-sm", onclick: move |_| on_goto.call(1), "管理订阅源" }
                    }
                }
            }
            div { class: "card",
                h2 { class: "card-title", "最近错误" }
                if error_rows.is_empty() {
                    div { class: "empty",
                        {icon("check", 36)}
                        span { class: "empty-title", "全部正常" }
                        span { class: "empty-hint", "最近一次合并没有失败源" }
                    }
                } else {
                    div { class: "warning-box", {error_rows.into_iter()} }
                }
            }
        }
    }
}

#[component]
fn StatCard(icon_name: &'static str, value: String, label: &'static str, danger: bool) -> Element {
    rsx! {
        div { class: "stat-card",
            div { class: if danger { "stat-icon danger" } else { "stat-icon" },
                {icon(icon_name, 18)}
            }
            div {
                div { class: "stat-value", "{value}" }
                div { class: "stat-label", "{label}" }
            }
        }
    }
}
