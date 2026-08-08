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
    "mixed-port: 7890\nallow-lan: false\nmode: rule\nlog-level: info\n\nipv6: false\n\ndns:\n  enable: true\n  listen: 0.0.0.0:53\n  ipv6: false\n  enhanced-mode: fake-ip\n  fake-ip-range: 198.18.0.1/16\n  fake-ip-filter:\n    - \"*.lan\"\n    - \"*.local\"\n    - \"+.msftconnecttest.com\"\n    - \"+.msftncsi.com\"\n    - \"time.*.com\"\n    - \"time.*.gov\"\n    - \"time.*.edu.cn\"\n    - \"*.ntp.org.cn\"\n    - \"*.pool.ntp.org\"\n    - \"time.cloudflare.com\"\n  nameserver:\n    - 119.29.29.29\n  fallback:\n    - 1.1.1.1\n  proxy-server-nameserver:\n    - 119.29.29.29\n  nameserver-policy:\n    \"geosite:cn\":\n      - 119.29.29.29\n  fallback-filter:\n    geoip: true\n    geoip-code: CN\n    ipcidr:\n      - 240.0.0.0/4\n\nrule-providers:\n  geosite-google:\n    type: http\n    behavior: domain\n    format: mrs\n    url: \"https://gh-proxy.org/https://github.com/MetaCubeX/meta-rules-dat/raw/meta/geo/geosite/google.mrs\"\n    path: ./ruleset/geosite-google.mrs\n    interval: 86400\n  geosite-youtube:\n    type: http\n    behavior: domain\n    format: mrs\n    url: \"https://gh-proxy.org/https://github.com/MetaCubeX/meta-rules-dat/raw/meta/geo/geosite/youtube.mrs\"\n    path: ./ruleset/geosite-youtube.mrs\n    interval: 86400\n  geosite-nodeseek:\n    type: http\n    behavior: domain\n    format: mrs\n    url: \"https://gh-proxy.org/https://github.com/MetaCubeX/meta-rules-dat/raw/meta/geo/geosite/nodeseek.mrs\"\n    path: ./ruleset/geosite-nodeseek.mrs\n    interval: 86400\n  geosite-cn:\n    type: http\n    behavior: domain\n    format: mrs\n    url: \"https://gh-proxy.org/https://github.com/MetaCubeX/meta-rules-dat/raw/meta/geo/geosite/cn.mrs\"\n    path: ./ruleset/geosite-cn.mrs\n    interval: 86400\n  geoip-cn:\n    type: http\n    behavior: ipcidr\n    format: mrs\n    url: \"https://gh-proxy.org/https://github.com/MetaCubeX/meta-rules-dat/raw/meta/geo/geoip/cn.mrs\"\n    path: ./ruleset/geoip-cn.mrs\n    interval: 86400\n\nrules:\n  - RULE-SET,geosite-google,节点选择\n  - RULE-SET,geosite-youtube,节点选择\n  - RULE-SET,geosite-nodeseek,节点选择\n  - RULE-SET,geosite-cn,DIRECT\n  - RULE-SET,geoip-cn,DIRECT\n  - MATCH,节点选择\n".to_string()
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
