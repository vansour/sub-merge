// crates/server/src/auth.rs
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::header::AUTHORIZATION;

/// 解析 Bearer 头。返回三态：Ok(token) / Err("missing authorization header"，无
/// Authorization 头) / Err("invalid authorization header"，非 UTF-8) /
/// Err("expected Bearer token"，前缀不符)。
pub fn parse_bearer(headers: &HeaderMap) -> Result<&str, &'static str> {
    let auth = headers
        .get(AUTHORIZATION)
        .ok_or("missing authorization header")?;
    let auth_str = auth.to_str().map_err(|_| "invalid authorization header")?;
    auth_str
        .strip_prefix("Bearer ")
        .ok_or("expected Bearer token")
}

/// 提取 Bearer token（require_admin 与 logout 复用），失败统一返回 None。
pub fn extract_bearer(headers: &HeaderMap) -> Option<&str> {
    parse_bearer(headers).ok()
}

/// 校验 Bearer 会话 token：sha256(token) 查 sessions 表。返回 Ok(()) 或 401。
pub async fn require_admin(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(), ApiError> {
    let token = match parse_bearer(&headers) {
        Ok(t) => t,
        Err(msg) => return Err(ApiError::unauthorized(msg)),
    };
    if crate::db::validate_session(&state.pool, token, state.cfg.session_ttl_days).await? {
        Ok(())
    } else {
        Err(ApiError::unauthorized("invalid session"))
    }
}

// axum 中间件式鉴权：把 require_admin 作为 before 层。
// 本方案采用在 handler 内显式调用的方式（简单直观），不引入 middleware 层。
