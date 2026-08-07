// crates/server/src/auth.rs
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::header::AUTHORIZATION;

/// 提取 Bearer token（require_admin 与 logout 复用）
pub fn extract_bearer(headers: &HeaderMap) -> Option<&str> {
    let auth = headers.get(AUTHORIZATION)?.to_str().ok()?;
    auth.strip_prefix("Bearer ")
}

/// 校验 Bearer 会话 token：sha256(token) 查 sessions 表。返回 Ok(()) 或 401。
pub async fn require_admin(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(), ApiError> {
    let Some(token) = extract_bearer(&headers) else {
        return Err(ApiError::unauthorized("missing authorization header"));
    };
    if crate::db::validate_session(&state.pool, token, state.cfg.session_ttl_days).await? {
        Ok(())
    } else {
        Err(ApiError::unauthorized("invalid session"))
    }
}

// axum 中间件式鉴权：把 require_admin 作为 before 层。
// 本方案采用在 handler 内显式调用的方式（简单直观），不引入 middleware 层。
