// crates/server/src/routes/config.rs
use crate::auth::require_admin;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

pub fn router() -> Router<AppState> {
    Router::new().route("/admin/config", get(get_config).put(rotate_config))
}

#[derive(Serialize)]
pub struct ConfigDto {
    pub admin_token: String,
}

#[derive(Deserialize)]
pub struct RotateConfig {
    pub rotate: Option<String>,
}

async fn config_dto(state: &AppState) -> Result<ConfigDto, ApiError> {
    let admin = crate::db::get_setting(&state.pool, "admin_token")
        .await?
        .unwrap_or_default();
    Ok(ConfigDto { admin_token: admin })
}

async fn get_config(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<ConfigDto>, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    Ok(Json(config_dto(&state).await?))
}

async fn rotate_config(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: Result<Json<RotateConfig>, JsonRejection>,
) -> Result<Json<ConfigDto>, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    let Json(body) = body.map_err(ApiError::from)?;
    match body.rotate.as_deref() {
        // 订阅 token 已移除；rotate 仅接受 admin。
        Some("admin") => {
            let t = crate::db::gen_token();
            crate::db::set_setting(&state.pool, "admin_token", &t).await?;
            state.rotate_admin(t).await;
        }
        Some(_) => {
            return Err(ApiError::bad_request("rotate must be 'admin'"));
        }
        None => {}
    }
    Ok(Json(config_dto(&state).await?))
}
