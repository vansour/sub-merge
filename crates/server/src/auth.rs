// crates/server/src/auth.rs
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::State;
use axum::http::header::AUTHORIZATION;
use axum::http::HeaderMap;

/// 校验 Bearer 管理 token。返回 Ok(()) 或 401。
pub async fn require_admin(State(state): State<AppState>, headers: HeaderMap) -> Result<(), ApiError> {
    let Some(auth) = headers.get(AUTHORIZATION) else {
        return Err(ApiError::unauthorized("missing authorization header"));
    };
    let Ok(auth_str) = auth.to_str() else {
        return Err(ApiError::unauthorized("invalid authorization header"));
    };
    let Some(token) = auth_str.strip_prefix("Bearer ") else {
        return Err(ApiError::unauthorized("expected Bearer token"));
    };
    if constant_eq(token, &*state.admin_token.read().await) {
        Ok(())
    } else {
        Err(ApiError::unauthorized("invalid admin token"))
    }
}

fn constant_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

// axum 中间件式鉴权：把 require_admin 作为 before 层。
// 本方案采用在 handler 内显式调用的方式（简单直观），不引入 middleware 层。
