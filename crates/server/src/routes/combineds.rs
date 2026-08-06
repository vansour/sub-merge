// crates/server/src/routes/combineds.rs
use crate::auth::require_admin;
use crate::error::ApiError;
use crate::routes::valid_combined_name;
use crate::state::AppState;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct CombinedDto {
    pub id: i64,
    pub name: String,
    pub created_at: String,
    // 成员 source_id 列表（服务端查询填充）
    pub source_ids: Vec<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CreateCombined {
    pub name: String,
    pub source_ids: Option<Vec<i64>>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateCombined {
    pub name: Option<String>,
    pub source_ids: Option<Vec<i64>>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/admin/combineds",
            get(list_combineds).post(create_combined),
        )
        .route(
            "/admin/combineds/{id}",
            put(update_combined).delete(delete_combined),
        )
}

/// SQLite UNIQUE 约束冲突（错误码 2067 / 消息含 UNIQUE constraint failed）
fn is_unique_violation(e: &sqlx::Error) -> bool {
    e.as_database_error()
        .map(|d| d.message().contains("UNIQUE"))
        .unwrap_or(false)
}

async fn member_ids(state: &AppState, combined_id: i64) -> Result<Vec<i64>, ApiError> {
    Ok(sqlx::query_scalar(
        "SELECT source_id FROM combined_sources WHERE combined_id = ? ORDER BY source_id",
    )
    .bind(combined_id)
    .fetch_all(&state.pool)
    .await?)
}

/// 批量插入成员：单条 INSERT...SELECT 同时过滤不存在的源（幂等，PK 冲突由 OR IGNORE 处理）。
/// 在调用方事务内执行（conn 传 &mut tx，自动 deref 到 SqliteConnection）；ids 为空直接返回（DELETE 已清空）。
async fn insert_members_sql(
    conn: &mut sqlx::SqliteConnection,
    combined_id: i64,
    source_ids: &[i64],
) -> Result<(), ApiError> {
    if source_ids.is_empty() {
        return Ok(());
    }
    let placeholders = std::iter::repeat_n("?", source_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let mut q = sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT OR IGNORE INTO combined_sources (combined_id, source_id) SELECT ?, id FROM sources WHERE id IN ({placeholders})"
    )));
    q = q.bind(combined_id);
    for sid in source_ids {
        q = q.bind(sid);
    }
    q.execute(conn).await?;
    Ok(())
}

fn dto(id: i64, name: String, created_at: String, source_ids: Vec<i64>) -> CombinedDto {
    CombinedDto {
        id,
        name,
        created_at,
        source_ids,
    }
}

async fn list_combineds(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Vec<CombinedDto>>, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    let rows: Vec<(i64, String, String)> =
        sqlx::query_as("SELECT id, name, created_at FROM combined_subs ORDER BY id")
            .fetch_all(&state.pool)
            .await?;
    // 成员一次查回并按 combined_id 分组，消除逐组合查询的 N+1
    let members: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT combined_id, source_id FROM combined_sources ORDER BY combined_id, source_id",
    )
    .fetch_all(&state.pool)
    .await?;
    let mut by_combined: HashMap<i64, Vec<i64>> = HashMap::new();
    for (cid, sid) in members {
        by_combined.entry(cid).or_default().push(sid);
    }
    let mut out = Vec::new();
    for (id, name, created_at) in rows {
        out.push(dto(
            id,
            name,
            created_at,
            by_combined.remove(&id).unwrap_or_default(),
        ));
    }
    Ok(Json(out))
}

async fn create_combined(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: Result<Json<CreateCombined>, JsonRejection>,
) -> Result<(axum::http::StatusCode, Json<CombinedDto>), ApiError> {
    require_admin(State(state.clone()), headers).await?;
    let Json(body) = body.map_err(ApiError::from)?;
    if !valid_combined_name(&body.name) {
        return Err(ApiError::bad_request(
            "combined name must match [A-Za-z0-9-_]",
        ));
    }
    let created_at = chrono::Utc::now().to_rfc3339();
    // 组合行 + 成员插入同一事务：任一失败整体回滚，不留"孤立组合行"窗口
    let mut tx = state.pool.begin().await?;
    let res = sqlx::query("INSERT INTO combined_subs (name, created_at) VALUES (?, ?)")
        .bind(&body.name)
        .bind(&created_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            if is_unique_violation(&e) {
                ApiError::bad_request("combined name already exists")
            } else {
                e.into()
            }
        })?;
    let id = res.last_insert_rowid();
    insert_members_sql(&mut tx, id, &body.source_ids.unwrap_or_default()).await?;
    tx.commit().await?;
    // 成员插入后从库中读回实际生效的 id：请求里不存在的源 id 被跳过，
    // 响应与列表/更新端点一致（只含真实成员）。
    let source_ids = member_ids(&state, id).await?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(dto(id, body.name, created_at, source_ids)),
    ))
}

async fn update_combined(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    id: Result<Path<i64>, PathRejection>,
    body: Result<Json<UpdateCombined>, JsonRejection>,
) -> Result<Json<CombinedDto>, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    let Path(id) = id.map_err(ApiError::from)?;
    let Json(body) = body.map_err(ApiError::from)?;

    // 组合必须存在（先查，区分 404 与校验 400）
    let (old_name, created_at) =
        sqlx::query_as("SELECT name, created_at FROM combined_subs WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| ApiError::not_found("combined subscription not found"))?;

    // 名字更新：校验 + 唯一性
    let name = match &body.name {
        Some(n) => {
            if !valid_combined_name(n) {
                return Err(ApiError::bad_request(
                    "combined name must match [A-Za-z0-9-_]",
                ));
            }
            if n != &old_name {
                sqlx::query("UPDATE combined_subs SET name = ? WHERE id = ?")
                    .bind(n)
                    .bind(id)
                    .execute(&state.pool)
                    .await
                    .map_err(|e| {
                        if is_unique_violation(&e) {
                            ApiError::bad_request("combined name already exists")
                        } else {
                            e.into()
                        }
                    })?;
            }
            n.clone()
        }
        None => old_name,
    };

    // 成员全量替换（事务：删除 + 插入，避免中间态）
    if let Some(ids) = &body.source_ids {
        let mut tx = state.pool.begin().await?;
        sqlx::query("DELETE FROM combined_sources WHERE combined_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        insert_members_sql(&mut tx, id, ids).await?;
        tx.commit().await?;
    }

    Ok(Json(dto(
        id,
        name,
        created_at,
        member_ids(&state, id).await?,
    )))
}

async fn delete_combined(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    id: Result<Path<i64>, PathRejection>,
) -> Result<axum::http::StatusCode, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    let Path(id) = id.map_err(ApiError::from)?;
    let res = sqlx::query("DELETE FROM combined_subs WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(ApiError::not_found("combined subscription not found"));
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}
