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
    Router::new().route("/admin/config", get(get_config).put(update_config))
}

#[derive(Serialize)]
pub struct ConfigDto {
    pub username: String,
}

#[derive(Deserialize)]
pub struct UpdateConfig {
    pub change_password: Option<ChangePassword>,
}

#[derive(Deserialize)]
pub struct ChangePassword {
    pub old: String,
    pub new: String,
}

async fn config_dto(state: &AppState) -> Result<ConfigDto, ApiError> {
    let username = crate::db::get_username(&state.pool)
        .await?
        .ok_or_else(|| ApiError::internal("no admin user"))?;
    Ok(ConfigDto { username })
}

async fn get_config(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<ConfigDto>, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    Ok(Json(config_dto(&state).await?))
}

async fn update_config(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: Result<Json<UpdateConfig>, JsonRejection>,
) -> Result<Json<ConfigDto>, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    let Json(b) = body.map_err(ApiError::from)?;
    if let Some(cp) = &b.change_password {
        let username = crate::db::get_username(&state.pool)
            .await?
            .ok_or_else(|| ApiError::internal("no admin user"))?;
        if !crate::db::verify_user(&state.pool, &username, &cp.old).await? {
            return Err(ApiError::bad_request("old password is incorrect"));
        }
        if cp.new.len() < 8 {
            return Err(ApiError::bad_request(
                "password must be at least 8 characters",
            ));
        }
        crate::db::update_password(&state.pool, &username, &cp.new).await?;
        // 修改密码后全部会话（含当前）立即失效
        crate::db::delete_all_sessions(&state.pool).await?;
    }
    Ok(Json(config_dto(&state).await?))
}
