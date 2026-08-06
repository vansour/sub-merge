// crates/server/src/routes/subscribe.rs
use crate::error::ApiError;
use crate::service;
use crate::state::AppState;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::response::Response;
use proxy_core::serializer::{OutputFormat, serialize_nodes};
use std::str::FromStr;

#[derive(serde::Deserialize)]
pub struct SubscribeQuery {
    pub format: Option<String>,
}

pub async fn subscribe_handler(
    State(state): State<AppState>,
    Path(name): Path<String>,
    q: Result<Query<SubscribeQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let Query(q) = q.map_err(ApiError::from)?;
    // 组合订阅名必须匹配 settings 中的 combined_name（缺省 merged）。
    // 不匹配 → 404（区别于 SPA 回退的 HTML 404，走统一 JSON 错误格式）。
    let combined = crate::db::get_setting(&state.pool, "combined_name")
        .await?
        .unwrap_or_else(|| "merged".to_string());
    if name != combined {
        return Err(ApiError::not_found("combined subscription not found"));
    }

    let format = match &q.format {
        Some(f) => {
            OutputFormat::from_str(f).map_err(|_| ApiError::bad_request("unsupported format"))?
        }
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
        return Err(ApiError::bad_gateway(format!(
            "all sources failed: {details}"
        )));
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
