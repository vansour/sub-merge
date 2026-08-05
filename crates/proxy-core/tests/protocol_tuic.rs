use proxy_core::model::Protocol;
use proxy_core::protocols::tuic::{parse_tuic, serialize_tuic};

const TUIC: &str = "tuic://11111111-2222-3333-4444-555555555555:pass%40word@1.2.3.4:443?congestion_control=bbr&udp_relay_mode=native&alpn=h3&sni=example.com&allow_insecure=1#TW-01";

#[test]
fn parse_tuic_basic() {
    let n = parse_tuic(TUIC).unwrap();
    assert_eq!(n.kind, Protocol::Tuic);
    assert_eq!(n.uuid.as_deref(), Some("11111111-2222-3333-4444-555555555555"));
    assert_eq!(n.password.as_deref(), Some("pass@word"));
    assert_eq!(n.server, "1.2.3.4");
    assert_eq!(n.port, 443);
    assert_eq!(n.name, "TW-01");
    let tls = n.tls.as_ref().unwrap();
    assert!(tls.enabled);
    assert!(tls.insecure);
    assert_eq!(tls.alpn, vec!["h3".to_string()]);
}

#[test]
fn tuic_roundtrip() {
    let n = parse_tuic(TUIC).unwrap();
    let out = serialize_tuic(&n).unwrap();
    assert!(out.starts_with("tuic://"));
    let n2 = parse_tuic(&out).unwrap();
    assert_eq!(n2.uuid, n.uuid);
    assert_eq!(n2.password, n.password);
    assert_eq!(n2.tls, n.tls);
}

#[test]
fn tuic_invalid() {
    assert!(parse_tuic("tuic://").is_err());
}
