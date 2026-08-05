use proxy_core::model::Protocol;
use proxy_core::protocols::hysteria::{parse_hysteria, serialize_hysteria};

const HY1: &str = "hysteria://1.2.3.4:36712?protocol=udp&auth=secret123&upmbps=100&downmbps=100&obfs=xorsalsa20&sni=example.com&insecure=1&alpn=hysteria#JP-01";

#[test]
fn parse_hysteria_basic() {
    let n = parse_hysteria(HY1).unwrap();
    assert_eq!(n.kind, Protocol::Hysteria);
    assert_eq!(n.server, "1.2.3.4");
    assert_eq!(n.port, 36712);
    assert_eq!(n.password.as_deref(), Some("secret123"));
    assert_eq!(n.name, "JP-01");
    let tls = n.tls.as_ref().unwrap();
    assert!(tls.enabled);
    assert!(tls.insecure);
    assert_eq!(tls.sni.as_deref(), Some("example.com"));
    assert_eq!(tls.alpn, vec!["hysteria".to_string()]);
}

#[test]
fn hysteria_roundtrip() {
    let n = parse_hysteria(HY1).unwrap();
    let out = serialize_hysteria(&n).unwrap();
    assert!(out.starts_with("hysteria://"));
    let n2 = parse_hysteria(&out).unwrap();
    assert_eq!(n2.password, n.password);
    assert_eq!(n2.server, n.server);
    assert_eq!(n2.tls, n.tls);
}

#[test]
fn hysteria_invalid() {
    assert!(parse_hysteria("hysteria://").is_err());
}
