use proxy_core::model::Protocol;
use proxy_core::protocols::trojan::{parse_trojan, serialize_trojan};

const TROJAN_TCP: &str = "trojan://pass%40word@1.2.3.4:443?security=tls&sni=example.com#KR-01";
const TROJAN_WS: &str = "trojan://abc123@1.2.3.4:443?security=tls&sni=example.com&type=ws&path=%2Ftr&host=example.com#KR-02";

#[test]
fn parse_trojan_tcp() {
    let n = parse_trojan(TROJAN_TCP).unwrap();
    assert_eq!(n.kind, Protocol::Trojan);
    assert_eq!(n.server, "1.2.3.4");
    assert_eq!(n.port, 443);
    assert_eq!(n.password.as_deref(), Some("pass@word"));
    assert_eq!(n.name, "KR-01");
    let tls = n.tls.as_ref().unwrap();
    assert!(tls.enabled);
    assert_eq!(tls.sni.as_deref(), Some("example.com"));
    assert!(n.transport.is_none());
}

#[test]
fn parse_trojan_ws() {
    let n = parse_trojan(TROJAN_WS).unwrap();
    let ws = n
        .transport
        .as_ref()
        .and_then(|t| t.websocket.as_ref())
        .unwrap();
    assert_eq!(ws.path, "/tr");
    assert_eq!(ws.host.as_deref(), Some("example.com"));
}

#[test]
fn trojan_roundtrip() {
    let n = parse_trojan(TROJAN_TCP).unwrap();
    let out = serialize_trojan(&n).unwrap();
    assert!(out.starts_with("trojan://"));
    let n2 = parse_trojan(&out).unwrap();
    assert_eq!(n2.password, n.password);
    assert_eq!(n2.server, n.server);
    assert_eq!(n2.tls, n.tls);
}

#[test]
fn trojan_httpupgrade_roundtrip() {
    let uri =
        "trojan://pass@1.2.3.4:443?security=tls&type=httpupgrade&path=%2Fup&host=example.com#KR-03";
    let n = parse_trojan(uri).unwrap();
    assert!(
        n.transport
            .as_ref()
            .and_then(|t| t.http_upgrade.as_ref())
            .is_some()
    );
    let out = serialize_trojan(&n).unwrap();
    let n2 = parse_trojan(&out).unwrap();
    assert!(
        n2.transport
            .as_ref()
            .and_then(|t| t.http_upgrade.as_ref())
            .is_some(),
        "httpupgrade transport lost in roundtrip: {out}"
    );
}

#[test]
fn trojan_invalid() {
    assert!(parse_trojan("trojan://").is_err());
}

#[test]
fn trojan_defaults_to_tls_without_security_param() {
    // 标准分享格式无 security 参数：trojan 协议强制 TLS 承载
    let n = parse_trojan("trojan://pass@1.2.3.4:443#T").unwrap();
    let tls = n
        .tls
        .expect("trojan without security param must default to TLS");
    assert!(tls.enabled);
    // 显式 security=none 仍然关闭 TLS（保持序列化修复的测试语义）
    let n2 = parse_trojan("trojan://pass@1.2.3.4:443?security=none#T").unwrap();
    assert!(n2.tls.is_none());
}

#[test]
fn trojan_serialize_transport_without_tls_emits_valid_query() {
    // security=none（显式关闭 TLS）+ ws 传输：query 必须以 ? 开头（回归：缺 ? 产出非法 URI）
    let n = parse_trojan(
        "trojan://pass@1.2.3.4:443?security=none&type=ws&path=%2Fws&host=cdn.example.com#T",
    )
    .unwrap();
    assert!(n.tls.is_none());
    assert!(
        n.transport
            .as_ref()
            .and_then(|t| t.websocket.as_ref())
            .is_some()
    );
    let out = serialize_trojan(&n).unwrap();
    assert!(out.contains("?type=ws"), "query must start with '?': {out}");
    assert!(
        out.contains("&host=cdn.example.com"),
        "params joined with &: {out}"
    );
    let n2 = parse_trojan(&out).unwrap();
    assert_eq!(n2.port, 443);
    let ws = n2
        .transport
        .as_ref()
        .and_then(|t| t.websocket.as_ref())
        .unwrap();
    assert_eq!(ws.path, "/ws");
}
