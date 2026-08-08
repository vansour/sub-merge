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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_template_is_valid_and_survives_subscription_serialization() {
        // 出厂 default_template 字面量必须能被 serialize_clash_subscription 消费：
        // 非法 YAML 或结构漂移会让 /subscribe 输出 500，只能靠此测试在 CI 兜住。
        let tpl = default_template();
        let out = proxy_core::formats::clash::serialize_clash_subscription(
            &tpl,
            "grp",
            "http://example.com/subscribe/grp?format=v2ray",
        )
        .unwrap();
        // 系统段：单组节点选择 + provider
        let v: serde_yaml_ng::Value = serde_yaml_ng::from_str(&out).unwrap();
        let groups = v["proxy-groups"].as_sequence().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0]["name"].as_str().unwrap(), "节点选择");
        assert_eq!(v["proxy-providers"].as_mapping().unwrap().len(), 1);
        // 模板段：dns + 5 个 rule-providers + 6 条 rules 全部存活
        assert!(v["dns"].is_mapping(), "dns 段保留");
        let providers = v["rule-providers"].as_mapping().unwrap();
        assert_eq!(providers.len(), 5, "5 个规则集保留");
        let rules = v["rules"].as_sequence().unwrap();
        assert_eq!(rules.len(), 6, "6 条规则保留");
        assert!(!out.contains("substore"));
    }

    #[test]
    fn default_template_matches_published_meta_shape() {
        // 锁定默认模板 = 发布配置（clash-vansour.meta.yaml 的模板段，未入库的 untracked
        // 参照文件）的关键形态，防止未来任一方漂移：
        // 1) 模板纯净——不含 proxy-providers/proxy-groups（系统段由订阅输出时自动追加）
        // 2) 6 条 rules 逐条存在且引用「节点选择」组
        // 3) 模板以 MATCH 规则结尾（发布形态：rules 段位于系统段之后）
        let tpl = default_template();
        assert!(
            !tpl.contains("proxy-providers:"),
            "模板不得含系统段 proxy-providers（由 serialize_clash_subscription 追加）"
        );
        assert!(
            !tpl.contains("proxy-groups:"),
            "模板不得含系统段 proxy-groups（由 serialize_clash_subscription 追加）"
        );
        for rule in [
            "RULE-SET,geosite-google,节点选择",
            "RULE-SET,geosite-youtube,节点选择",
            "RULE-SET,geosite-nodeseek,节点选择",
            "RULE-SET,geosite-cn,DIRECT",
            "RULE-SET,geoip-cn,DIRECT",
            "MATCH,节点选择",
        ] {
            assert!(tpl.contains(rule), "默认模板缺少规则: {rule}");
        }
        assert!(
            tpl.trim_end().ends_with("MATCH,节点选择"),
            "模板必须以 MATCH 规则结尾（发布形态：rules 段在末尾）"
        );
    }
}
