// crates/server/src/routes/stats.rs
use crate::auth::require_admin;
use crate::error::ApiError;
use crate::service;
use crate::state::AppState;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize)]
pub struct StatsDto {
    pub total_nodes: usize,
    pub protocol_counts: BTreeMap<String, usize>,
    pub errors: Vec<String>,
    pub sources: usize,
    pub kinds: BTreeMap<String, i64>,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/admin/stats", get(stats_handler))
}

async fn stats_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<StatsDto>, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    let (nodes, source_errors) = service::fetch_and_merge(&state, None).await;

    let mut protocol_counts: BTreeMap<String, usize> = BTreeMap::new();
    for n in &nodes {
        *protocol_counts
            .entry(n.kind.as_str().to_string())
            .or_default() += 1;
    }

    // kinds 统计不拉网络：一次聚合查询
    let rows: Vec<(String, i64)> =
        sqlx::query_as("SELECT kind, COUNT(*) FROM sources GROUP BY kind")
            .fetch_all(&state.pool)
            .await
            .map_err(ApiError::from)?;
    let mut kinds: BTreeMap<String, i64> = BTreeMap::new();
    for (k, c) in rows {
        kinds.insert(k, c);
    }

    let errors: Vec<String> = source_errors
        .iter()
        .map(|e| format!("{}: {}", e.source_name, e.reason))
        .collect();

    Ok(Json(StatsDto {
        total_nodes: nodes.len(),
        protocol_counts,
        errors,
        sources: kinds.values().sum::<i64>() as usize,
        kinds,
    }))
}
