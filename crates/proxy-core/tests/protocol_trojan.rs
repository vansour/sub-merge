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
    let ws = n.transport.as_ref().and_then(|t| t.websocket.as_ref()).unwrap();
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
    let uri = "trojan://pass@1.2.3.4:443?security=tls&type=httpupgrade&path=%2Fup&host=example.com#KR-03";
    let n = parse_trojan(uri).unwrap();
    assert!(n.transport.as_ref().and_then(|t| t.http_upgrade.as_ref()).is_some());
    let out = serialize_trojan(&n).unwrap();
    let n2 = parse_trojan(&out).unwrap();
    assert!(
        n2.transport.as_ref().and_then(|t| t.http_upgrade.as_ref()).is_some(),
        "httpupgrade transport lost in roundtrip: {out}"
    );
}

#[test]
fn trojan_invalid() {
    assert!(parse_trojan("trojan://").is_err());
}
