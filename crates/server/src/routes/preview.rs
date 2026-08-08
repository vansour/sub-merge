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
    pub kind: Option<String>,
    pub source_id: Option<i64>,
}

async fn preview_handler(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    q: Result<Query<PreviewQuery>, QueryRejection>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    let Query(q) = q.map_err(ApiError::from)?;
    if [
        q.kind.is_some(),
        q.combined.is_some(),
        q.source_id.is_some(),
    ]
    .into_iter()
    .filter(|b| *b)
    .count()
        > 1
    {
        return Err(ApiError::bad_request(
            "kind/combined/source_id are mutually exclusive",
        ));
    }
    if let Some(k) = &q.kind
        && !matches!(k.as_str(), "single" | "remote")
    {
        return Err(ApiError::bad_request("kind must be 'single' or 'remote'"));
    }
    // 成员过滤：combined（按组合）→ kind（按类型）→ source_id（按单源）→ None（全部）
    let member_ids: Option<Vec<i64>> = if let Some(name) = &q.combined {
        let cid: Option<i64> = sqlx::query_scalar("SELECT id FROM combined_subs WHERE name = ?")
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
    } else if let Some(k) = &q.kind {
        Some(
            sqlx::query_scalar("SELECT id FROM sources WHERE kind = ? ORDER BY id")
                .bind(k)
                .fetch_all(&state.pool)
                .await?,
        )
    } else if let Some(sid) = q.source_id {
        let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM sources WHERE id = ?")
            .bind(sid)
            .fetch_optional(&state.pool)
            .await?;
        if exists.is_none() {
            return Err(ApiError::not_found("source not found"));
        }
        Some(vec![sid])
    } else {
        None
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
