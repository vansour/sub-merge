use proxy_core::model::Protocol;
use proxy_core::protocols::wireguard::{parse_wireguard, serialize_wireguard};

const WG: &str = "wireguard://cHVibGljS2V5MTIz@1.2.3.4:443?publicKey=cHVibGljS2V5MTIz&privateKey=cHJpdmF0ZUtleTEyMw==&reserved=0,0,0&mtu=1420&ip=10.0.0.1%2F24,fd00::1%2F64#SG-01";

#[test]
fn parse_wireguard_basic() {
    let n = parse_wireguard(WG).unwrap();
    assert_eq!(n.kind, Protocol::Wireguard);
    assert_eq!(n.server, "1.2.3.4");
    assert_eq!(n.port, 443);
    assert_eq!(n.name, "SG-01");
    // uuid 字段存放 privateKey
    assert_eq!(n.uuid.as_deref(), Some("cHJpdmF0ZUtleTEyMw=="));
}

#[test]
fn wireguard_roundtrip() {
    let n = parse_wireguard(WG).unwrap();
    let out = serialize_wireguard(&n).unwrap();
    assert!(out.starts_with("wireguard://"));
    let n2 = parse_wireguard(&out).unwrap();
    assert_eq!(n2.uuid, n.uuid);
    assert_eq!(n2.server, n.server);
}

#[test]
fn wireguard_invalid() {
    assert!(parse_wireguard("wireguard://").is_err());
}
