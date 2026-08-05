// crates/server/web/src/components/preview.rs
// Task 4：转换预览。拉取 /api/admin/preview 渲染节点表 + 源错误，支持手动刷新。
use crate::api::request;
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

#[component]
pub fn Preview(token: Signal<Option<String>>) -> Element {
    let data = use_signal(|| None::<PreviewResp>);
    let loading = use_signal(|| false);
    let error = use_signal(String::new);

    // 初次挂载时加载一次。
    // 用 use_future（挂载时只跑一次），避免计划里的 spawn-on-render 模式
    // 在每次 render 时重复发起请求。
    use_future(move || {
        let token = token.read().clone();
        let mut data = data;
        let mut loading = loading;
        let mut error = error;
        async move {
            loading.set(true);
            match request("GET", "/api/admin/preview", None, token.as_deref()).await {
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
            match request("GET", "/api/admin/preview", None, token.as_deref()).await {
                Ok(body) => match serde_json::from_str::<PreviewResp>(&body) {
                    Ok(r) => data.set(Some(r)),
                    Err(e) => error.set(format!("解析失败: {}", e)),
                },
                Err(e) => error.set(e),
            }
            loading.set(false);
        });
    };

    rsx! {
        div { class: "card",
            h2 { "转换预览" }
            if !error.read().is_empty() {
                p { style: "color: #ff3b30", "{error}" }
            }
            button { onclick: reload, "刷新预览" }
            if *loading.read() {
                p { "加载中..." }
            }
            if let Some(resp) = data.read().as_ref() {
                p { "共 {resp.total} 个节点" }
                table {
                    thead { tr { th { "名称" } th { "协议" } th { "服务器" } th { "端口" } } }
                    tbody {
                        for n in &resp.nodes {
                            tr {
                                td { "{n.name}" }
                                td { "{n.protocol}" }
                                td { "{n.server}" }
                                td { "{n.port}" }
                            }
                        }
                    }
                }
                if !resp.errors.is_empty() {
                    h4 { "源错误" }
                    ul {
                        for e in &resp.errors {
                            li { "{e}" }
                        }
                    }
                }
            }
        }
    }
}
