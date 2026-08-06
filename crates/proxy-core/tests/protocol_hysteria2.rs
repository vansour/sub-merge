use proxy_core::model::Protocol;
use proxy_core::protocols::hysteria2::{parse_hysteria2, serialize_hysteria2};

const HY2: &str = "hysteria2://pass%40word@1.2.3.4:8443?sni=example.com&insecure=0&obfs=salamander&obfs-password=obs#US-02";

#[test]
fn parse_hysteria2_basic() {
    let n = parse_hysteria2(HY2).unwrap();
    assert_eq!(n.kind, Protocol::Hysteria2);
    assert_eq!(n.server, "1.2.3.4");
    assert_eq!(n.port, 8443);
    assert_eq!(n.password.as_deref(), Some("pass@word"));
    assert_eq!(n.name, "US-02");
    let tls = n.tls.as_ref().unwrap();
    assert!(tls.enabled);
    assert_eq!(tls.sni.as_deref(), Some("example.com"));
    assert!(!tls.insecure);
}

#[test]
fn hysteria2_insecure_flag() {
    let n = parse_hysteria2("hysteria2://p@1.2.3.4:8443?insecure=1#T").unwrap();
    assert!(n.tls.as_ref().unwrap().insecure);
}

#[test]
fn hysteria2_roundtrip() {
    let n = parse_hysteria2(HY2).unwrap();
    let out = serialize_hysteria2(&n).unwrap();
    assert!(out.starts_with("hysteria2://"));
    let n2 = parse_hysteria2(&out).unwrap();
    assert_eq!(n2.password, n.password);
    assert_eq!(n2.server, n.server);
    assert_eq!(n2.tls, n.tls);
}

#[test]
fn hysteria2_invalid() {
    assert!(parse_hysteria2("hysteria2://").is_err());
}

#[test]
fn hysteria2_serialize_without_sni_emits_valid_query() {
    // alpn/insecure 存在但 sni 为 None 时，query 必须以 ? 开头（回归：缺 ? 产出非法 URI）
    let n = parse_hysteria2("hysteria2://pass@1.2.3.4:8443?alpn=h3&insecure=1#T").unwrap();
    assert!(n.tls.as_ref().unwrap().sni.is_none());
    let out = serialize_hysteria2(&n).unwrap();
    assert!(out.contains("?alpn=h3"), "query must start with '?': {out}");
    assert!(out.contains("&insecure=1"), "params joined with &: {out}");
    // 输出必须能被自己解析
    let n2 = parse_hysteria2(&out).unwrap();
    assert_eq!(n2.server, "1.2.3.4");
    assert_eq!(n2.port, 8443);
    assert_eq!(n2.tls.as_ref().unwrap().alpn, vec!["h3".to_string()]);
    assert!(n2.tls.as_ref().unwrap().insecure);
}
