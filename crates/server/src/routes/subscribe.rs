// crates/server/src/routes/subscribe.rs
// 组合订阅输出：仅 v2ray 格式（clash 支持已于 2026-08-08 移除——订阅组模式/模板/
// proxy-providers 全部删除，format=clash 显式 400）。
use crate::error::ApiError;
use crate::service;
use crate::state::AppState;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::response::Response;
use proxy_core::formats::v2ray::{serialize_v2ray, serialize_v2ray_plain};

#[derive(serde::Deserialize)]
pub struct SubscribeQuery {
    pub format: Option<String>,
}

/// 合法格式值（v2ray 别名容错，与原 OutputFormat::from_str 的 v2ray 分支一致）。
fn valid_v2ray_format(f: &str) -> bool {
    matches!(f.to_ascii_lowercase().as_str(), "v2ray" | "v2r" | "base64")
}

pub async fn subscribe_handler(
    State(state): State<AppState>,
    Path(name): Path<String>,
    q: Result<Query<SubscribeQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let Query(q) = q.map_err(ApiError::from)?;
    // clash 已移除：显式 400 提示（含别名），避免客户端拿到错误输出后困惑。
    if let Some(f) = &q.format
        && matches!(
            f.to_ascii_lowercase().as_str(),
            "clash" | "clashyaml" | "yaml"
        )
    {
        return Err(ApiError::bad_request(
            "clash format has been removed; use format=v2ray",
        ));
    }
    if let Some(f) = &q.format
        && !valid_v2ray_format(f)
    {
        return Err(ApiError::bad_request("unsupported format"));
    }
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

    // 按后台「订阅输出」设置选形态：base64（默认）或纯 URI 文本行。
    let body = if crate::db::get_setting(&state.pool, crate::routes::config::V2RAY_B64_KEY)
        .await?
        .as_deref()
        != Some("0")
    {
        serialize_v2ray(&nodes).map_err(|e| ApiError::internal(e.to_string()))?
    } else {
        serialize_v2ray_plain(&nodes).map_err(|e| ApiError::internal(e.to_string()))?
    };

    // text/plain：浏览器直接渲染（不触发下载）；客户端解析 body 内容，不依赖 content-type。
    let resp = Response::builder()
        .header("content-type", "text/plain; charset=utf-8")
        .header("profile-update-interval", "24")
        .body(axum::body::Body::from(body))
        .unwrap();
    Ok(resp)
}
