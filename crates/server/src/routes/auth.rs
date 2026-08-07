// crates/server/src/routes/auth.rs
use crate::auth::extract_bearer;
use crate::error::ApiError;
use crate::routes::is_unique_violation;
use crate::state::AppState;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct SetupRequest {
    pub username: String,
    pub password: String,
    pub password_confirm: String,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct LoginResp {
    pub token: String,
}

#[derive(Serialize)]
pub struct SetupStatusResp {
    pub needs_setup: bool,
}

fn valid_username(s: &str) -> bool {
    let t = s.trim();
    !t.is_empty()
        && t.len() <= 64
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/setup-status", get(setup_status))
        .route("/admin/setup", post(setup))
        .route("/admin/login", post(login))
        .route("/admin/logout", post(logout))
}

async fn setup_status(State(state): State<AppState>) -> Result<Json<SetupStatusResp>, ApiError> {
    let needs_setup = crate::db::users_empty(&state.pool).await?;
    Ok(Json(SetupStatusResp { needs_setup }))
}

async fn setup(
    State(state): State<AppState>,
    body: Result<Json<SetupRequest>, JsonRejection>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let Json(b) = body.map_err(ApiError::from)?;
    let username = b.username.trim().to_string();
    if !valid_username(&username) {
        return Err(ApiError::bad_request(
            "username must match [A-Za-z0-9-_] (1-64 chars)",
        ));
    }
    if b.password.len() < 8 {
        return Err(ApiError::bad_request(
            "password must be at least 8 characters",
        ));
    }
    if b.password != b.password_confirm {
        return Err(ApiError::bad_request("passwords do not match"));
    }
    // 原子创建（INSERT...SELECT WHERE NOT EXISTS 单语句）：并发 setup 不可能产生双管理员
    let created = crate::db::create_user_if_empty(&state.pool, &username, &b.password).await;
    match created {
        Ok(true) => Ok(Json(serde_json::json!({ "username": username }))),
        Ok(false) => Err(ApiError::conflict("admin user already exists")),
        // UNIQUE 兜底：极端竞态（同用户名并发）下 INSERT 冲突也转 409（非 500）
        Err(e) => {
            if e.downcast_ref::<sqlx::Error>()
                .map(is_unique_violation)
                .unwrap_or(false)
            {
                Err(ApiError::conflict("admin user already exists"))
            } else {
                Err(e.into())
            }
        }
    }
}

async fn login(
    State(state): State<AppState>,
    body: Result<Json<LoginRequest>, JsonRejection>,
) -> Result<Json<LoginResp>, ApiError> {
    let Json(b) = body.map_err(ApiError::from)?;
    if !crate::db::verify_user(&state.pool, b.username.trim(), &b.password).await? {
        return Err(ApiError::unauthorized("invalid username or password"));
    }
    let token = crate::db::create_session(&state.pool).await?;
    Ok(Json(LoginResp { token }))
}

async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<axum::http::StatusCode, ApiError> {
    let Some(token) = extract_bearer(&headers) else {
        return Err(ApiError::unauthorized("missing authorization header"));
    };
    // logout 删除会话即注销；token 不存在也返回 204（幂等）
    crate::db::delete_session(&state.pool, token).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}
