use proxy_core::model::Protocol;
use proxy_core::protocols::vless::{parse_vless, serialize_vless};

const VLESS_WS: &str = "vless://11111111-2222-3333-4444-555555555555@1.2.3.4:443?encryption=none&security=tls&sni=cdn.example.com&type=ws&path=%2Fws&fp=chrome#JP-01";
const VLESS_TCP: &str = "vless://11111111-2222-3333-4444-555555555555@1.2.3.4:443?encryption=none&security=none&type=tcp#JP-01";

#[test]
fn parse_vless_ws_tls() {
    let n = parse_vless(VLESS_WS).unwrap();
    assert_eq!(n.kind, Protocol::Vless);
    assert_eq!(n.uuid.as_deref(), Some("11111111-2222-3333-4444-555555555555"));
    assert_eq!(n.server, "1.2.3.4");
    assert_eq!(n.port, 443);
    assert_eq!(n.name, "JP-01");
    let tls = n.tls.as_ref().unwrap();
    assert!(tls.enabled);
    assert_eq!(tls.sni.as_deref(), Some("cdn.example.com"));
    assert_eq!(tls.fingerprint.as_deref(), Some("chrome"));
    let ws = n.transport.as_ref().and_then(|t| t.websocket.as_ref()).unwrap();
    assert_eq!(ws.path, "/ws");
}

#[test]
fn parse_vless_tcp_none() {
    let n = parse_vless(VLESS_TCP).unwrap();
    assert!(n.tls.is_none());
    assert!(n.transport.is_none());
}

#[test]
fn vless_roundtrip() {
    let n = parse_vless(VLESS_WS).unwrap();
    let out = serialize_vless(&n).unwrap();
    assert!(out.starts_with("vless://"));
    let n2 = parse_vless(&out).unwrap();
    assert_eq!(n2.uuid, n.uuid);
    assert_eq!(n2.server, n.server);
    assert_eq!(n2.port, n.port);
    assert_eq!(n2.transport, n.transport);
    assert_eq!(n2.tls, n.tls);
}

#[test]
fn vless_invalid() {
    assert!(parse_vless("vless://").is_err());
    assert!(parse_vless("vless://bad@host").is_err());
}
