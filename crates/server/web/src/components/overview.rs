// crates/server/web/src/components/overview.rs
// 概览页：4 张统计卡片（源总数/启用中/节点总数/失败源数）+ 订阅源摘要 + 最近错误。
// 数据来自 DataStore 缓存（MainShell 预载，本页刷新按钮触发重拉），纯客户端聚合。
use crate::components::icon::{icon, Spinner};
use crate::data::{CacheStatus, DataStore, UnitKey};
use dioxus::prelude::*;

#[component]
pub fn Overview(token: Signal<Option<String>>, on_goto: EventHandler<usize>) -> Element {
    // token 已由 DataStore 内部持有；签名保留以兼容 MainShell 调用点。
    let _ = token;
    let data = use_context::<DataStore>();
    let sources_state = data.sources.read().clone();
    let preview_state = data.preview.read().clone();

    // 刷新：DataStore 重拉对应单元（加载期间保留旧 data，按钮即刻转圈）。
    let refreshing = sources_state.status == CacheStatus::Loading
        || preview_state.status == CacheStatus::Loading;
    let reload = move |_| {
        data.refresh(UnitKey::Sources);
        data.refresh(UnitKey::Preview);
    };

    // 统计值在 rsx 外预计算（避免借用冲突）。
    let source_total = sources_state.data.as_ref().map(|s| s.len()).unwrap_or(0);
    let enabled_count = sources_state
        .data
        .as_ref()
        .map(|s| s.iter().filter(|s| s.enabled).count())
        .unwrap_or(0);
    let (node_total, failed_count) = preview_state
        .data
        .as_ref()
        .map(|s| (s.total, s.errors.len()))
        .unwrap_or((0, 0));

    let source_rows: Vec<Element> = sources_state
        .data
        .as_ref()
        .map(|list| {
            list.iter()
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
                .collect()
        })
        .unwrap_or_default();

    let error_rows: Vec<Element> = preview_state
        .data
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

    let page_error = [sources_state.error.clone(), preview_state.error.clone()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("; ");

    let on_goto = on_goto.clone();
    rsx! {
        div { class: "page-head",
            h1 { class: "page-title", "概览" }
            button { class: "btn btn-secondary", onclick: reload, disabled: refreshing,
                if refreshing {
                    Spinner { size: 14 }
                } else {
                    {icon("refresh", 14)}
                }
                "刷新"
            }
        }
        if !page_error.is_empty() {
            p { class: "error-text", "{page_error}" }
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
