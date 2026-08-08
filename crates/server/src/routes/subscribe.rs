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
    headers: axum::http::HeaderMap,
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

    // clash 分支：订阅组模式（不再解析节点输出）——模板 + proxy-providers 引用本服务的
    // v2ray 聚合订阅链接。组合名作为 provider key。
    if format == OutputFormat::Clash {
        let scheme = headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .filter(|s| s == &"https")
            .map(|_| "https")
            .unwrap_or("http");
        let host = headers
            .get(axum::http::header::HOST)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| ApiError::bad_request("missing Host header"))?;
        let provider_url = format!("{scheme}://{host}/subscribe/{name}?format=v2ray");
        let template =
            crate::db::get_setting(&state.pool, crate::routes::clash_config::TEMPLATE_KEY)
                .await?
                .unwrap_or_else(crate::routes::clash_config::default_template);
        let body = proxy_core::formats::clash::serialize_clash_subscription(
            &template,
            &name,
            &provider_url,
        )
        .map_err(|e| ApiError::internal(e.to_string()))?;
        return Ok(Response::builder()
            .header("content-type", "text/plain; charset=utf-8")
            .header("profile-update-interval", "24")
            .body(axum::body::Body::from(body))
            .unwrap());
    }

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

    // v2ray 分支按设置选输出形态：base64（默认）或纯 URI 文本行。
    // 显式 match：未来新增格式不会静默落入 v2ray-plain 输出。
    let body = match format {
        OutputFormat::Clash => unreachable!("clash 分支已早返回（订阅组模式）"),
        OutputFormat::V2ray => {
            if crate::db::get_setting(&state.pool, crate::routes::config::V2RAY_B64_KEY)
                .await?
                .as_deref()
                != Some("0")
            {
                serialize_nodes(&nodes, format).map_err(|e| ApiError::internal(e.to_string()))?
            } else {
                proxy_core::formats::v2ray::serialize_v2ray_plain(&nodes)
                    .map_err(|e| ApiError::internal(e.to_string()))?
            }
        }
    };

    // text/plain：浏览器直接渲染（不触发下载）；mihomo/clash 客户端拉取订阅
    // 时解析 body 内容，不依赖 content-type。
    let content_type = match format {
        OutputFormat::Clash => "text/plain; charset=utf-8",
        OutputFormat::V2ray => "text/plain; charset=utf-8",
    };

    let resp = Response::builder()
        .header("content-type", content_type)
        .header("profile-update-interval", "24")
        .body(axum::body::Body::from(body))
        .unwrap();
    Ok(resp)
}
