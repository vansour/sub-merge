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
    pub combined_name: String,
    pub subscribe_url: String,
}

#[derive(Deserialize)]
pub struct RotateConfig {
    pub rotate: Option<String>,
    pub combined_name: Option<String>,
}

/// 组合订阅名：路径段安全（无 URL 编码），限定 [A-Za-z0-9-_]
fn valid_combined_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

async fn config_dto(state: &AppState) -> Result<ConfigDto, ApiError> {
    let admin = crate::db::get_setting(&state.pool, "admin_token")
        .await?
        .unwrap_or_default();
    let combined_name = crate::db::get_setting(&state.pool, "combined_name")
        .await?
        .unwrap_or_else(|| "merged".to_string());
    Ok(ConfigDto {
        admin_token: admin,
        subscribe_url: format!("/subscribe/{}", combined_name),
        combined_name,
    })
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
    // 先整体校验、后统一应用：请求非法（rotate 值非法或 combined_name 不合法）时
    // 不得产生任何副作用（尤其是 rotate 会轮换 admin token）。
    if let Some(n) = &body.combined_name
        && !valid_combined_name(n)
    {
        return Err(ApiError::bad_request(
            "combined_name must match [A-Za-z0-9-_]",
        ));
    }
    // 应用顺序：先写无害字段（combined_name），再执行破坏性轮换。任一步写库失败
    // 返回 500 时，绝不能出现"admin token 已轮换但响应丢失"的锁死状态。
    if let Some(n) = &body.combined_name {
        crate::db::set_setting(&state.pool, "combined_name", n).await?;
    }
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
