// crates/server/src/routes/config.rs
use crate::auth::require_admin;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

pub fn router() -> Router<AppState> {
    Router::new().route("/api/admin/config", get(get_config).put(rotate_config))
}

#[derive(Serialize)]
pub struct ConfigDto {
    pub subscribe_token: String,
    pub admin_token: String,
    pub subscribe_url: String,
}

#[derive(Deserialize)]
pub struct RotateConfig {
    pub rotate: Option<String>,
}

async fn get_config(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<ConfigDto>, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    let sub = crate::db::get_setting(&state.pool, "subscribe_token").await?.unwrap_or_default();
    let admin = crate::db::get_setting(&state.pool, "admin_token").await?.unwrap_or_default();
    Ok(Json(ConfigDto {
        subscribe_token: sub,
        admin_token: admin,
        subscribe_url: "/api/subscribe".to_string(),
    }))
}

async fn rotate_config(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<RotateConfig>,
) -> Result<Json<ConfigDto>, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    match body.rotate.as_deref() {
        Some("subscribe") => {
            let t = crate::db::gen_token();
            crate::db::set_setting(&state.pool, "subscribe_token", &t).await?;
        }
        Some("admin") => {
            let t = crate::db::gen_token();
            crate::db::set_setting(&state.pool, "admin_token", &t).await?;
            // 注意：轮换 admin token 后，旧 token 立即失效。本请求用旧 token 调用已通过校验。
        }
        Some(_) => return Err(ApiError::bad_request("rotate must be 'subscribe' or 'admin'")),
        None => {}
    }
    let sub = crate::db::get_setting(&state.pool, "subscribe_token").await?.unwrap_or_default();
    let admin = crate::db::get_setting(&state.pool, "admin_token").await?.unwrap_or_default();
    Ok(Json(ConfigDto {
        subscribe_token: sub,
        admin_token: admin,
        subscribe_url: "/api/subscribe".to_string(),
    }))
}
