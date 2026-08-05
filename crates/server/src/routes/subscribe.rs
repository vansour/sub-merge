// crates/server/src/routes/subscribe.rs
use crate::error::ApiError;
use crate::service;
use crate::state::AppState;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::response::Response;
use proxy_core::serializer::{serialize_nodes, OutputFormat};
use sqlx::Row;
use std::str::FromStr;

#[derive(serde::Deserialize)]
pub struct SubscribeQuery {
    pub token: Option<String>,
    pub format: Option<String>,
}

pub async fn subscribe_handler(
    State(state): State<AppState>,
    q: Result<Query<SubscribeQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let Query(q) = q.map_err(ApiError::from)?;
    // 校验订阅 token
    let stored = sqlx::query("SELECT value FROM settings WHERE key = 'subscribe_token'")
        .fetch_optional(&state.pool)
        .await
        .map_err(ApiError::from)?;
    let Some(row) = stored else {
        return Err(ApiError::internal("subscribe token not initialized"));
    };
    let stored: String = row.get(0);
    let Some(tok) = &q.token else {
        return Err(ApiError::unauthorized("missing subscribe token"));
    };
    // 恒定时间比较
    if !constant_eq(tok, &stored) {
        return Err(ApiError::unauthorized("invalid subscribe token"));
    }

    let format = match &q.format {
        Some(f) => OutputFormat::from_str(f).map_err(|_| ApiError::bad_request("unsupported format"))?,
        None => OutputFormat::Clash,
    };

    let (nodes, source_errors) = service::fetch_and_merge(&state).await;

    // 若所有源都失败，返回 502 附明细
    if nodes.is_empty() && !source_errors.is_empty() {
        let details = source_errors
            .iter()
            .map(|e| format!("{}: {}", e.source_name, e.reason))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(ApiError::bad_gateway(format!("all sources failed: {details}")));
    }

    let body = serialize_nodes(&nodes, format).map_err(|e| ApiError::internal(e.to_string()))?;

    let content_type = match format {
        OutputFormat::Clash => "application/x-yaml",
        OutputFormat::V2ray => "text/plain; charset=utf-8",
        OutputFormat::Singbox => "application/json",
    };

    let resp = Response::builder()
        .header("content-type", content_type)
        .header("profile-update-interval", "24")
        .body(axum::body::Body::from(body))
        .unwrap();
    Ok(resp)
}

/// 恒定时间字符串比较，防时序侧信道。
fn constant_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}
