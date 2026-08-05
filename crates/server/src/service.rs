// crates/server/src/service.rs
use crate::state::AppState;
use proxy_core::model::ProxyNode;
use proxy_core::parser::parse_subscription_text;
use sqlx::Row;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinSet;

#[derive(Debug, Clone)]
pub struct SourceError {
    pub source_name: String,
    pub reason: String,
}

/// 并发拉取全部 enabled 源，解析合并。返回 (节点, 错误源列表)。
pub async fn fetch_and_merge(state: &AppState) -> (Vec<ProxyNode>, Vec<SourceError>) {
    let sources: Vec<(i64, String, String)> = sqlx::query("SELECT id, name, url FROM sources WHERE enabled = 1")
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|r| (r.get::<i64, _>(0), r.get::<String, _>(1), r.get::<String, _>(2)))
        .collect();

    let max_nodes = state.cfg.max_nodes;
    let client = Arc::new(state.http.clone());
    let timeout = Duration::from_secs(state.cfg.timeout_secs);

    let mut set = JoinSet::new();
    for (_, name, url) in sources {
        let client = client.clone();
        set.spawn(async move {
            match fetch_source(&client, &url, timeout).await {
                Ok(text) => {
                    let (nodes, _skipped) = parse_subscription_text(&text, max_nodes);
                    (Some(name), nodes)
                }
                Err(_reason) => (Some(name), Vec::new()),
            }
        });
    }

    let mut all_nodes = Vec::new();
    let mut errors = Vec::new();
    while let Some(res) = set.join_next().await {
        let Ok((name, mut nodes)) = res else { continue };
        if let Some(n) = name {
            if nodes.is_empty() {
                errors.push(SourceError { source_name: n, reason: "no nodes parsed or fetch failed".into() });
            } else {
                all_nodes.append(&mut nodes);
            }
        }
    }
    // 上限截断
    all_nodes.truncate(max_nodes);
    (all_nodes, errors)
}

pub async fn fetch_source(client: &reqwest::Client, url: &str, timeout: Duration) -> Result<String, String> {
    let resp = client
        .get(url)
        .timeout(timeout)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("http status {}", resp.status()));
    }
    resp.text()
        .await
        .map_err(|e| format!("read body failed: {e}"))
}
