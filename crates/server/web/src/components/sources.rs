// crates/server/web/src/components/sources.rs
// Task 3：订阅源 CRUD + 刷新。与后端 /api/admin/sources 交互。
use crate::api::request;
use dioxus::prelude::*;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct SourceDto {
    pub id: i64,
    pub url: String,
    pub name: String,
    pub enabled: bool,
    // 后端返回的字段，作为 API 契约保留；UI 表格暂不展示。
    #[allow(dead_code)]
    pub created_at: String,
}

pub async fn fetch_sources(token: Option<&str>) -> Result<Vec<SourceDto>, String> {
    let body = request("GET", "/api/admin/sources", None, token).await?;
    serde_json::from_str(&body).map_err(|e| format!("解析失败: {}", e))
}

#[component]
pub fn Sources(token: Signal<Option<String>>) -> Element {
    let sources = use_signal(Vec::<SourceDto>::new);
    let mut error = use_signal(String::new);
    let mut new_url = use_signal(String::new);
    let mut new_name = use_signal(String::new);

    // 初次挂载时加载一次列表。
    // 用 use_future（只在挂载时跑一次），避免计划里的 spawn-on-render 模式
    // 在每次 render 时重复发起请求。
    use_future(move || {
        let token = token.read().clone();
        let mut sources = sources;
        let mut error = error;
        async move {
            match request("GET", "/api/admin/sources", None, token.as_deref()).await {
                Ok(body) => {
                    if let Ok(list) = serde_json::from_str::<Vec<SourceDto>>(&body) {
                        sources.set(list);
                    }
                }
                Err(e) => error.set(e),
            }
        }
    });

    let add = move |_| {
        let url = new_url.read().clone();
        let name = new_name.read().clone();
        if url.is_empty() || name.is_empty() {
            error.set("URL 和名称不能为空".into());
            return;
        }
        let token = token.read().clone();
        let body = serde_json::json!({ "url": url, "name": name }).to_string();
        let mut sources = sources.clone();
        let mut new_url = new_url.clone();
        let mut new_name = new_name.clone();
        spawn(async move {
            match request("POST", "/api/admin/sources", Some(body), token.as_deref()).await {
                Ok(_) => {
                    // 重新加载
                    if let Ok(body) = request("GET", "/api/admin/sources", None, token.as_deref()).await {
                        if let Ok(list) = serde_json::from_str::<Vec<SourceDto>>(&body) {
                            sources.set(list);
                        }
                    }
                    new_url.set(String::new());
                    new_name.set(String::new());
                }
                Err(e) => error.set(e),
            }
        });
    };

    let toggle = move |id: i64, enabled: bool| {
        let token = token.read().clone();
        let body = serde_json::json!({ "enabled": !enabled }).to_string();
        let mut sources = sources.clone();
        spawn(async move {
            let _ = request("PUT", &format!("/api/admin/sources/{id}"), Some(body), token.as_deref()).await;
            if let Ok(body) = request("GET", "/api/admin/sources", None, token.as_deref()).await {
                if let Ok(list) = serde_json::from_str::<Vec<SourceDto>>(&body) {
                    sources.set(list);
                }
            }
        });
    };

    let del = move |id: i64| {
        let token = token.read().clone();
        let mut sources = sources.clone();
        spawn(async move {
            let _ = request("DELETE", &format!("/api/admin/sources/{id}"), None, token.as_deref()).await;
            if let Ok(body) = request("GET", "/api/admin/sources", None, token.as_deref()).await {
                if let Ok(list) = serde_json::from_str::<Vec<SourceDto>>(&body) {
                    sources.set(list);
                }
            }
        });
    };

    let refresh = move |id: i64| {
        let token = token.read().clone();
        spawn(async move {
            let _ = request("POST", &format!("/api/admin/sources/{id}/refresh"), None, token.as_deref()).await;
        });
    };

    // 先把行渲染成 owned Element，避免 sources.read() 临时借用被 map 惰性迭代器拖出作用域
    // （E0716）。在 move 闭包里只捕获 Copy 的 id/enabled。
    let rows: Vec<Element> = sources
        .read()
        .iter()
        .map(|s| {
            let id = s.id;
            let enabled = s.enabled;
            rsx! {
                tr {
                    td { "{s.id}" }
                    td { "{s.name}" }
                    td { "{s.url}" }
                    td {
                        span { class: format!("badge {}", if enabled { "on" } else { "off" }),
                            if enabled { "启用" } else { "停用" }
                        }
                    }
                    td {
                        button { class: "secondary", onclick: move |_| toggle(id, enabled),
                            if enabled { "停用" } else { "启用" }
                        }
                        button { class: "secondary", onclick: move |_| refresh(id), "刷新" }
                        button { class: "danger", onclick: move |_| del(id), "删除" }
                    }
                }
            }
        })
        .collect();

    rsx! {
        div { class: "card",
            h2 { "订阅源" }
            if !error.read().is_empty() {
                p { style: "color: #ff3b30", "{error}" }
            }
            div {
                input { placeholder: "订阅 URL", value: new_url, oninput: move |e| new_url.set(e.value()) }
                input { placeholder: "名称", value: new_name, oninput: move |e| new_name.set(e.value()) }
                button { onclick: add, "添加" }
            }
            table {
                thead {
                    tr { th { "ID" } th { "名称" } th { "URL" } th { "状态" } th { "操作" } }
                }
                tbody {
                    {rows.into_iter()}
                }
            }
        }
    }
}
