// 与 server 的 API 契约 DTO。fixture 取自 crates/server/src/routes/*.rs 实际输出形状。
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SourceDto {
    pub id: i64,
    pub url: String,
    pub name: String,
    pub kind: String,
    // 后端返回的字段，作为 API 契约保留；UI 暂不展示。
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CombinedDto {
    pub id: i64,
    pub name: String,
    // 后端返回的字段，作为 API 契约保留；UI 暂不展示。
    pub created_at: String,
    pub source_ids: Vec<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PreviewNode {
    pub name: String,
    pub protocol: String,
    pub server: String,
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PreviewResp {
    pub nodes: Vec<PreviewNode>,
    pub errors: Vec<String>,
    pub total: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConfigDto {
    pub username: String,
    pub v2ray_base64: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClashConfigDto {
    pub template: String,
}

// /admin/stats 响应（Task 1 后端聚合端点：total_nodes/protocol_counts/errors/sources/kinds）。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StatsDto {
    pub total_nodes: usize,
    pub protocol_counts: std::collections::BTreeMap<String, usize>,
    pub errors: Vec<String>,
    pub sources: usize,
    pub kinds: std::collections::BTreeMap<String, i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_dto_parses_full_fields() {
        let j = r#"{"id":1,"url":"https://example.com/sub","name":"机场A","kind":"remote","created_at":"2026-08-07 12:00:00"}"#;
        let d: SourceDto = serde_json::from_str(j).unwrap();
        assert_eq!(d.id, 1);
        assert_eq!(d.url, "https://example.com/sub");
        assert_eq!(d.name, "机场A");
        assert_eq!(d.kind, "remote");
        assert_eq!(d.created_at, "2026-08-07 12:00:00");
    }

    #[test]
    fn source_dto_single_kind() {
        let j = r#"{"id":2,"url":"ss://a@b:8388","name":"单条","kind":"single","created_at":"2026-08-07 12:00:00"}"#;
        let d: SourceDto = serde_json::from_str(j).unwrap();
        assert_eq!(d.kind, "single");
    }

    #[test]
    fn combined_dto_parses_source_ids() {
        let j = r#"{"id":1,"name":"home","created_at":"2026-08-07 12:00:00","source_ids":[1,2]}"#;
        let d: CombinedDto = serde_json::from_str(j).unwrap();
        assert_eq!(d.id, 1);
        assert_eq!(d.name, "home");
        assert_eq!(d.source_ids, vec![1, 2]);
    }

    #[test]
    fn preview_resp_parses_full_shape() {
        let j = r#"{"nodes":[{"name":"节点1","protocol":"vmess","server":"1.2.3.4","port":443}],"errors":["源A: 超时"],"total":1}"#;
        let d: PreviewResp = serde_json::from_str(j).unwrap();
        assert_eq!(d.nodes.len(), 1);
        assert_eq!(d.nodes[0].name, "节点1");
        assert_eq!(d.nodes[0].protocol, "vmess");
        assert_eq!(d.nodes[0].server, "1.2.3.4");
        assert_eq!(d.nodes[0].port, 443);
        assert_eq!(d.errors, vec!["源A: 超时"]);
        assert_eq!(d.total, 1);
    }

    #[test]
    fn preview_resp_empty() {
        let j = r#"{"nodes":[],"errors":[],"total":0}"#;
        let d: PreviewResp = serde_json::from_str(j).unwrap();
        assert!(d.nodes.is_empty());
        assert!(d.errors.is_empty());
        assert_eq!(d.total, 0);
    }

    #[test]
    fn config_dto_parses() {
        let j = r#"{"username":"admin","v2ray_base64":true}"#;
        let d: ConfigDto = serde_json::from_str(j).unwrap();
        assert_eq!(d.username, "admin");
        assert!(d.v2ray_base64);
    }

    #[test]
    fn clash_config_dto_parses() {
        let j = r#"{"template":"mixed-port: 7890\nmode: rule"}"#;
        let d: ClashConfigDto = serde_json::from_str(j).unwrap();
        assert_eq!(d.template, "mixed-port: 7890\nmode: rule");
    }

    #[test]
    fn stats_dto_parses_full_shape() {
        let j = r#"{"total_nodes":320,"protocol_counts":{"vmess":150,"ss":120},"errors":["源A: 超时"],"sources":5,"kinds":{"single":2,"remote":3}}"#;
        let d: StatsDto = serde_json::from_str(j).unwrap();
        assert_eq!(d.total_nodes, 320);
        assert_eq!(d.protocol_counts["vmess"], 150);
        assert_eq!(d.errors, vec!["源A: 超时"]);
        assert_eq!(d.sources, 5);
        assert_eq!(d.kinds["remote"], 3);
    }

    #[test]
    fn stats_dto_empty() {
        let j = r#"{"total_nodes":0,"protocol_counts":{},"errors":[],"sources":0,"kinds":{}}"#;
        let d: StatsDto = serde_json::from_str(j).unwrap();
        assert_eq!(d.total_nodes, 0);
        assert!(d.protocol_counts.is_empty());
        assert!(d.kinds.is_empty());
    }

    #[test]
    fn unknown_fields_ignored() {
        let j = r#"{"id":1,"url":"x","name":"n","kind":"remote","enabled":true,"created_at":"t","extra":"ignored"}"#;
        let d: SourceDto = serde_json::from_str(j).unwrap();
        assert_eq!(d.name, "n");
    }
}
