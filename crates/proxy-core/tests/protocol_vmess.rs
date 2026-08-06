use proxy_core::model::Protocol;
use proxy_core::protocols::vmess::{parse_vmess, serialize_vmess};

// 对应 JSON: {"v":"2","ps":"SG-01","add":"1.2.3.4","port":"443","id":"uuid-1111-1111","aid":"0","net":"ws","type":"none","host":"cdn.example.com","path":"/ws","tls":"tls"}
const VMESS: &str = "vmess://eyJ2IjoiMiIsInBzIjoiU0ctMDEiLCJhZGQiOiIxLjIuMy40IiwicG9ydCI6IjQ0MyIsImlkIjoidXVpZC0xMTExLTExMTEiLCJhaWQiOiIwIiwibmV0Ijoid3MiLCJ0eXBlIjoibm9uZSIsImhvc3QiOiJjZG4uZXhhbXBsZS5jb20iLCJwYXRoIjoiL3dzIiwidGxzIjoidGxzIn0=";

#[test]
fn parse_vmess_ws_tls() {
    let n = parse_vmess(VMESS).unwrap();
    assert_eq!(n.kind, Protocol::Vmess);
    assert_eq!(n.name, "SG-01");
    assert_eq!(n.server, "1.2.3.4");
    assert_eq!(n.port, 443);
    assert_eq!(n.uuid.as_deref(), Some("uuid-1111-1111"));
    assert_eq!(n.alter_id, Some(0));
    let tls = n.tls.as_ref().unwrap();
    assert!(tls.enabled);
    assert_eq!(tls.sni.as_deref(), Some("cdn.example.com"));
    let ws = n
        .transport
        .as_ref()
        .and_then(|t| t.websocket.as_ref())
        .unwrap();
    assert_eq!(ws.path, "/ws");
}

#[test]
fn parse_vmess_tcp_none() {
    // {"v":"2","ps":"T","add":"1.2.3.4","port":"443","id":"u","aid":"0","net":"tcp","tls":"none"}
    let uri = "vmess://eyJ2IjoiMiIsInBzIjoiVCIsImFkZCI6IjEuMi4zLjQiLCJwb3J0IjoiNDQzIiwiaWQiOiJ1IiwiYWlkIjoiMCIsIm5ldCI6InRjcCIsInRscyI6Im5vbmUifQ==";
    let n = parse_vmess(uri).unwrap();
    assert!(n.tls.is_none());
    assert!(n.transport.is_none());
}

#[test]
fn vmess_roundtrip() {
    let n = parse_vmess(VMESS).unwrap();
    let out = serialize_vmess(&n).unwrap();
    assert!(out.starts_with("vmess://"));
    let n2 = parse_vmess(&out).unwrap();
    assert_eq!(n2.server, n.server);
    assert_eq!(n2.port, n.port);
    assert_eq!(n2.uuid, n.uuid);
    assert_eq!(n2.transport, n.transport);
    assert_eq!(n2.tls, n.tls);
}

#[test]
fn vmess_invalid() {
    assert!(parse_vmess("vmess://").is_err());
    assert!(parse_vmess("vmess://bm90LXN0YW5kYXJkLWpzb24=").is_err()); // "not-standard-json"
}

// 对应 JSON: {"v":"2","ps":"T","add":"1.2.3.4","port":"443","id":"u","aid":"0","net":"ws","path":"/ws","tls":"tls","allowInsecure":true}
const VMESS_BOOL_INSECURE: &str = "vmess://eyJ2IjoiMiIsInBzIjoiVCIsImFkZCI6IjEuMi4zLjQiLCJwb3J0IjoiNDQzIiwiaWQiOiJ1IiwiYWlkIjoiMCIsIm5ldCI6IndzIiwicGF0aCI6Ii93cyIsInRscyI6InRscyIsImFsbG93SW5zZWN1cmUiOnRydWV9";

// 对应 JSON: {"v":"2","ps":"T","add":"1.2.3.4","port":"443","id":"u","aid":"0","net":"ws","path":"/ws","tls":"tls"} (no host)
const VMESS_NO_HOST: &str = "vmess://eyJ2IjoiMiIsInBzIjoiVCIsImFkZCI6IjEuMi4zLjQiLCJwb3J0IjoiNDQzIiwiaWQiOiJ1IiwiYWlkIjoiMCIsIm5ldCI6IndzIiwicGF0aCI6Ii93cyIsInRscyI6InRscyJ9";

#[test]
fn parse_vmess_allow_insecure_bool() {
    let n = parse_vmess(VMESS_BOOL_INSECURE).unwrap();
    let tls = n.tls.as_ref().unwrap();
    assert!(tls.enabled);
    assert!(tls.insecure);
    let ws = n
        .transport
        .as_ref()
        .and_then(|t| t.websocket.as_ref())
        .unwrap();
    assert_eq!(ws.path, "/ws");
}

#[test]
fn vmess_roundtrip_no_host() {
    let n = parse_vmess(VMESS_NO_HOST).unwrap();
    assert!(n.tls.as_ref().unwrap().enabled);
    assert!(n.tls.as_ref().unwrap().sni.is_none());
    assert!(
        n.transport
            .as_ref()
            .and_then(|t| t.websocket.as_ref())
            .unwrap()
            .host
            .is_none()
    );
    let out = serialize_vmess(&n).unwrap();
    assert!(out.starts_with("vmess://"));
    let n2 = parse_vmess(&out).unwrap();
    assert_eq!(n2.server, n.server);
    assert_eq!(n2.port, n.port);
    assert_eq!(n2.transport, n.transport);
    assert_eq!(n2.tls, n.tls);
}
