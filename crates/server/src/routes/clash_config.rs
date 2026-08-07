// crates/server/src/routes/clash_config.rs
use crate::auth::require_admin;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

pub(crate) const TEMPLATE_KEY: &str = "clash_template";

pub(crate) fn default_template() -> String {
    "mixed-port: 7890\nallow-lan: false\nmode: rule\nlog-level: info\n\nrules:\n  - MATCH,🚀 节点选择\n".to_string()
}

pub fn router() -> Router<AppState> {
    Router::new().route("/admin/clash-config", get(get_config).put(put_config))
}

#[derive(Serialize)]
pub struct ClashConfigDto {
    pub template: String,
}

#[derive(Deserialize)]
pub struct UpdateClashConfig {
    pub template: String,
}

async fn get_config(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<ClashConfigDto>, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    let template = crate::db::get_setting(&state.pool, TEMPLATE_KEY)
        .await?
        .unwrap_or_else(default_template);
    Ok(Json(ClashConfigDto { template }))
}

async fn put_config(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: Result<Json<UpdateClashConfig>, JsonRejection>,
) -> Result<Json<ClashConfigDto>, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    let Json(b) = body.map_err(ApiError::from)?;
    // YAML 合法性校验（根必须是 mapping——serialize_clash_subscription 会再次校验）
    let v: serde_yaml_ng::Value = serde_yaml_ng::from_str(&b.template)
        .map_err(|e| ApiError::bad_request(format!("invalid YAML: {e}")))?;
    if !v.is_mapping() {
        return Err(ApiError::bad_request("template must be a YAML mapping"));
    }
    crate::db::set_setting(&state.pool, TEMPLATE_KEY, &b.template).await?;
    Ok(Json(ClashConfigDto {
        template: b.template,
    }))
}
