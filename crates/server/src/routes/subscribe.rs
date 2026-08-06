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
    // 组合订阅名必须匹配 combined_subs 表；不匹配 → 404
    let combined: Option<(i64, String)> =
        sqlx::query_as("SELECT id, name FROM combined_subs WHERE name = ?")
            .bind(&name)
            .fetch_optional(&state.pool)
            .await
            .map_err(ApiError::from)?;
    let Some((combined_id, _)) = combined else {
        return Err(ApiError::not_found("combined subscription not found"));
    };
    // 成员源 id 列表（空成员 → 空输出，200）
    let member_ids: Vec<i64> = sqlx::query_scalar(
        "SELECT source_id FROM combined_sources WHERE combined_id = ? ORDER BY source_id",
    )
    .bind(combined_id)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::from)?;

    let format = match &q.format {
        Some(f) => {
            OutputFormat::from_str(f).map_err(|_| ApiError::bad_request("unsupported format"))?
        }
        None => OutputFormat::Clash,
    };

    let (nodes, source_errors) = service::fetch_and_merge(&state, Some(&member_ids)).await;

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
