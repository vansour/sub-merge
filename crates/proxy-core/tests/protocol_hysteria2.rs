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
