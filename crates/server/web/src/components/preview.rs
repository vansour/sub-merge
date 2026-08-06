// crates/server/web/src/components/preview.rs
// 转换预览：节点表（协议彩色徽章）+ 源错误警告卡片。
use crate::api::request;
use crate::components::icon::{icon, Spinner};
use dioxus::prelude::*;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
struct PreviewNode {
    name: String,
    protocol: String,
    server: String,
    port: u16,
}

#[derive(Debug, Clone, Deserialize)]
struct PreviewResp {
    nodes: Vec<PreviewNode>,
    errors: Vec<String>,
    total: usize,
}

// 协议 → 配色（CSS --proto-0..5）。同族协议同色。
fn proto_class(protocol: &str) -> &'static str {
    match protocol {
        "ss" | "ssr" => "proto-0",
        "vmess" | "vless" => "proto-1",
        "trojan" => "proto-2",
        "hysteria" | "hysteria2" => "proto-3",
        "tuic" => "proto-4",
        _ => "proto-5",
    }
}

#[component]
pub fn Preview(token: Signal<Option<String>>) -> Element {
    let data = use_signal(|| None::<PreviewResp>);
    let loading = use_signal(|| false);
    let error = use_signal(String::new);

    // 初次挂载加载一次。
    use_future(move || {
        let token = token.read().clone();
        let mut data = data;
        let mut loading = loading;
        let mut error = error;
        async move {
            loading.set(true);
            error.set(String::new());
            match request("GET", "/admin/preview", None, token.as_deref()).await {
                Ok(body) => match serde_json::from_str::<PreviewResp>(&body) {
                    Ok(r) => data.set(Some(r)),
                    Err(e) => error.set(format!("解析失败: {}", e)),
                },
                Err(e) => error.set(e),
            }
            loading.set(false);
        }
    });

    let reload = move |_| {
        let token = token.read().clone();
        let mut data = data.clone();
        let mut loading = loading.clone();
        let mut error = error.clone();
        spawn(async move {
            loading.set(true);
            error.set(String::new());
            match request("GET", "/admin/preview", None, token.as_deref()).await {
                Ok(body) => match serde_json::from_str::<PreviewResp>(&body) {
                    Ok(r) => data.set(Some(r)),
                    Err(e) => error.set(format!("解析失败: {}", e)),
                },
                Err(e) => error.set(e),
            }
            loading.set(false);
        });
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

    rsx! {
        div { class: "page-head",
            h1 { class: "page-title", "转换预览" }
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
