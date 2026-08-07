// 与 server 的 API 契约 DTO。fixture 取自 crates/server/src/routes/*.rs 实际输出形状。
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct SourceDto {
    pub id: i64,
    pub url: String,
    pub name: String,
    pub kind: String,
    pub enabled: bool,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_dto_parses_full_fields() {
        let j = r#"{"id":1,"url":"https://example.com/sub","name":"机场A","kind":"remote","enabled":true,"created_at":"2026-08-07 12:00:00"}"#;
        let d: SourceDto = serde_json::from_str(j).unwrap();
        assert_eq!(d.id, 1);
        assert_eq!(d.url, "https://example.com/sub");
        assert_eq!(d.name, "机场A");
        assert_eq!(d.kind, "remote");
        assert!(d.enabled);
        assert_eq!(d.created_at, "2026-08-07 12:00:00");
    }

    #[test]
    fn source_dto_single_kind() {
        let j = r#"{"id":2,"url":"ss://a@b:8388","name":"单条","kind":"single","enabled":false,"created_at":"2026-08-07 12:00:00"}"#;
        let d: SourceDto = serde_json::from_str(j).unwrap();
        assert_eq!(d.kind, "single");
        assert!(!d.enabled);
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
        let j = r#"{"username":"admin"}"#;
        let d: ConfigDto = serde_json::from_str(j).unwrap();
        assert_eq!(d.username, "admin");
    }

    #[test]
    fn unknown_fields_ignored() {
        let j = r#"{"id":1,"url":"x","name":"n","kind":"remote","enabled":true,"created_at":"t","extra":"ignored"}"#;
        let d: SourceDto = serde_json::from_str(j).unwrap();
        assert_eq!(d.name, "n");
    }
}
