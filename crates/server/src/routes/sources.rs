// crates/server/src/routes/sources.rs
use crate::auth::require_admin;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SourceDto {
    pub id: i64,
    pub url: String,
    pub name: String,
    pub enabled: bool,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateSource {
    pub url: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSource {
    pub url: Option<String>,
    pub name: Option<String>,
    pub enabled: Option<bool>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/sources", get(list_sources).post(create_source))
        .route(
            "/admin/sources/{id}",
            put(update_source).delete(delete_source),
        )
        .route("/admin/sources/{id}/refresh", post(refresh_source))
}

async fn list_sources(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Vec<SourceDto>>, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    let rows = sqlx::query_as::<_, SourceDto>(
        "SELECT id, url, name, enabled, created_at FROM sources ORDER BY id",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

async fn create_source(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: Result<Json<CreateSource>, JsonRejection>,
) -> Result<(axum::http::StatusCode, Json<SourceDto>), ApiError> {
    require_admin(State(state.clone()), headers).await?;
    let Json(body) = body.map_err(ApiError::from)?;
    if body.url.is_empty() || body.name.is_empty() {
        return Err(ApiError::bad_request("url and name required"));
    }
    let created_at = chrono::Utc::now().to_rfc3339(); // 或手写时间
    let res =
        sqlx::query("INSERT INTO sources (url, name, enabled, created_at) VALUES (?, ?, 1, ?)")
            .bind(&body.url)
            .bind(&body.name)
            .bind(&created_at)
            .execute(&state.pool)
            .await?;
    let id = res.last_insert_rowid();
    let dto = SourceDto {
        id,
        url: body.url,
        name: body.name,
        enabled: true,
        created_at,
    };
    Ok((axum::http::StatusCode::CREATED, Json(dto)))
}

async fn update_source(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    id: Result<Path<i64>, PathRejection>,
    body: Result<Json<UpdateSource>, JsonRejection>,
) -> Result<Json<SourceDto>, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    let Path(id) = id.map_err(ApiError::from)?;
    let Json(body) = body.map_err(ApiError::from)?;
    // 先取现有
    let existing = sqlx::query_as::<_, SourceDto>(
        "SELECT id, url, name, enabled, created_at FROM sources WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("source not found"))?;

    let url = body.url.clone().unwrap_or(existing.url.clone());
    let name = body.name.clone().unwrap_or(existing.name.clone());
    let enabled = body.enabled.unwrap_or(existing.enabled);

    sqlx::query("UPDATE sources SET url = ?, name = ?, enabled = ? WHERE id = ?")
        .bind(&url)
        .bind(&name)
        .bind(enabled)
        .bind(id)
        .execute(&state.pool)
        .await?;

    let dto = SourceDto {
        id,
        url,
        name,
        enabled,
        created_at: existing.created_at,
    };
    Ok(Json(dto))
}

async fn delete_source(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    id: Result<Path<i64>, PathRejection>,
) -> Result<axum::http::StatusCode, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    let Path(id) = id.map_err(ApiError::from)?;
    let res = sqlx::query("DELETE FROM sources WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(ApiError::not_found("source not found"));
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}

async fn refresh_source(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    id: Result<Path<i64>, PathRejection>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    let Path(id) = id.map_err(ApiError::from)?;
    // 实时拉取模式下，refresh 即对该源重新抓取并报告结果
    let source = sqlx::query_as::<_, SourceDto>(
        "SELECT id, url, name, enabled, created_at FROM sources WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("source not found"))?;

    let result = crate::service::fetch_source(
        &state.http,
        &source.url,
        std::time::Duration::from_secs(state.cfg.timeout_secs),
    )
    .await;
    match result {
        Ok(text) => {
            let (nodes, _skipped) =
                proxy_core::parser::parse_subscription_text(&text, state.cfg.max_nodes);
            // 与 fetch_and_merge 一致：抓取成功但解析出 0 个节点视为源错误。
            if nodes.is_empty() {
                return Ok(Json(serde_json::json!({
                    "source": source.name,
                    "ok": false,
                    "reason": "no nodes parsed",
                })));
            }
            Ok(Json(serde_json::json!({
                "source": source.name,
                "ok": true,
                "node_count": nodes.len(),
            })))
        }
        Err(reason) => Ok(Json(serde_json::json!({
            "source": source.name,
            "ok": false,
            "reason": reason,
        }))),
    }
}
