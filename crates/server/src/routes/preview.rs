// crates/server/src/routes/preview.rs
use crate::auth::require_admin;
use crate::error::ApiError;
use crate::service;
use crate::state::AppState;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;

pub fn router() -> Router<AppState> {
    Router::new().route("/admin/preview", get(preview_handler))
}

#[derive(serde::Deserialize)]
pub struct PreviewQuery {
    pub combined: Option<String>,
}

async fn preview_handler(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    q: Result<Query<PreviewQuery>, QueryRejection>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    let Query(q) = q.map_err(ApiError::from)?;
    // combined 参数：按组合成员过滤；省略 → 全部 enabled 源
    let member_ids: Option<Vec<i64>> = match &q.combined {
        Some(name) => {
            let cid: Option<i64> =
                sqlx::query_scalar("SELECT id FROM combined_subs WHERE name = ?")
                    .bind(name)
                    .fetch_optional(&state.pool)
                    .await?;
            let Some(cid) = cid else {
                return Err(ApiError::not_found("combined subscription not found"));
            };
            Some(
                sqlx::query_scalar(
                    "SELECT source_id FROM combined_sources WHERE combined_id = ? ORDER BY source_id",
                )
                .bind(cid)
                .fetch_all(&state.pool)
                .await?,
            )
        }
        None => None,
    };
    let (nodes, errors) = service::fetch_and_merge(&state, member_ids.as_deref()).await;
    let node_list: Vec<serde_json::Value> = nodes
        .iter()
        .map(|n| {
            json!({
                "name": n.name,
                "protocol": n.kind.as_str(),
                "server": n.server,
                "port": n.port,
            })
        })
        .collect();
    let error_list: Vec<String> = errors
        .iter()
        .map(|e| format!("{}: {}", e.source_name, e.reason))
        .collect();
    Ok(Json(json!({
        "nodes": node_list,
        "errors": error_list,
        "total": nodes.len(),
    })))
}
