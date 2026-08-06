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

/// 并发拉取全部 enabled 源（受 cfg.concurrency 上限约束），解析合并。
/// 返回 (节点, 错误源列表)。
pub async fn fetch_and_merge(state: &AppState) -> (Vec<ProxyNode>, Vec<SourceError>) {
    let sources: Vec<(i64, String, String, String)> =
        sqlx::query("SELECT id, kind, name, url FROM sources WHERE enabled = 1")
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|r| {
                (
                    r.get::<i64, _>(0),
                    r.get::<String, _>(1),
                    r.get::<String, _>(2),
                    r.get::<String, _>(3),
                )
            })
            .collect();

    let max_nodes = state.cfg.max_nodes;
    let client = Arc::new(state.http.clone());
    let timeout = Duration::from_secs(state.cfg.timeout_secs);
    // 全局信号量（AppState 共享）：跨请求限制整个进程的并发拉取数，
    // 无鉴权订阅端点被并发请求时不会放大出站流量。
    let semaphore = state.fetch_semaphore.clone();

    let mut set = JoinSet::new();
    for (_, kind, name, url) in sources {
        // spawn 前获取信号量许可：同一时刻最多 cap 个源并发拉取。
        let Ok(permit) = semaphore.clone().acquire_owned().await else {
            continue; // 信号量被关闭，跳过该源
        };
        let client = client.clone();
        set.spawn(async move {
            let _permit = permit; // 任务期间持有许可
            let result = if kind == "single" {
                // 单条节点：直接解析，不发起网络请求
                proxy_core::parser::parse_line(&url)
                    .map(|n| vec![n])
                    .map_err(|e| format!("parse failed: {e}"))
            } else {
                match fetch_source(&client, &url, timeout).await {
                    Ok(text) => {
                        let (nodes, skipped) = parse_subscription_text(&text, max_nodes);
                        if nodes.is_empty() {
                            Err(format!("no nodes parsed ({} line(s) skipped)", skipped))
                        } else {
                            Ok(nodes)
                        }
                    }
                    Err(reason) => Err(reason),
                }
            };
            match result {
                Ok(nodes) => (name, Ok(nodes)),
                Err(reason) => (name, Err(reason)),
            }
        });
    }

    let mut all_nodes = Vec::new();
    let mut errors = Vec::new();
    while let Some(res) = set.join_next().await {
        let Ok((name, result)) = res else { continue };
        match result {
            Ok(mut nodes) => all_nodes.append(&mut nodes),
            Err(reason) => errors.push(SourceError {
                source_name: name,
                reason,
            }),
        }
    }
    // 上限截断
    all_nodes.truncate(max_nodes);
    (all_nodes, errors)
}

pub async fn fetch_source(
    client: &reqwest::Client,
    url: &str,
    timeout: Duration,
) -> Result<String, String> {
    let resp = client
        .get(url)
        .timeout(timeout)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("http status {}", resp.status()));
    }
    // 流式限长读取：边读边截断到 MAX_BODY_BYTES 即停，避免整包缓冲（resp.bytes() 会把
    // 整个响应体收进内存后才检查长度，超大响应可致内存暴涨）。超过上限的部分丢弃。
    const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;
    use futures_util::StreamExt;
    let mut stream = resp.bytes_stream();
    let mut buf = Vec::with_capacity(16 * 1024);
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("read body failed: {e}"))?;
        if buf.len() + chunk.len() > MAX_BODY_BYTES {
            let remaining = MAX_BODY_BYTES - buf.len();
            buf.extend_from_slice(&chunk[..remaining]);
            break;
        }
        buf.extend_from_slice(&chunk);
    }
    String::from_utf8(buf).map_err(|_| "body is not valid utf-8".to_string())
}
