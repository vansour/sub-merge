use proxy_core::model::Protocol;
use proxy_core::protocols::vless::{parse_vless, serialize_vless};

const VLESS_WS: &str = "vless://11111111-2222-3333-4444-555555555555@1.2.3.4:443?encryption=none&security=tls&sni=cdn.example.com&type=ws&path=%2Fws&fp=chrome#JP-01";
const VLESS_TCP: &str = "vless://11111111-2222-3333-4444-555555555555@1.2.3.4:443?encryption=none&security=none&type=tcp#JP-01";

#[test]
fn parse_vless_ws_tls() {
    let n = parse_vless(VLESS_WS).unwrap();
    assert_eq!(n.kind, Protocol::Vless);
    assert_eq!(
        n.uuid.as_deref(),
        Some("11111111-2222-3333-4444-555555555555")
    );
    assert_eq!(n.server, "1.2.3.4");
    assert_eq!(n.port, 443);
    assert_eq!(n.name, "JP-01");
    let tls = n.tls.as_ref().unwrap();
    assert!(tls.enabled);
    assert_eq!(tls.sni.as_deref(), Some("cdn.example.com"));
    assert_eq!(tls.fingerprint.as_deref(), Some("chrome"));
    let ws = n
        .transport
        .as_ref()
        .and_then(|t| t.websocket.as_ref())
        .unwrap();
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
fn vless_httpupgrade_roundtrip() {
    let uri = "vless://11111111-2222-3333-4444-555555555555@1.2.3.4:443?encryption=none&security=tls&type=httpupgrade&path=%2Fup&host=cdn.example.com#JP-02";
    let n = parse_vless(uri).unwrap();
    assert!(
        n.transport
            .as_ref()
            .and_then(|t| t.http_upgrade.as_ref())
            .is_some()
    );
    let out = serialize_vless(&n).unwrap();
    let n2 = parse_vless(&out).unwrap();
    assert!(
        n2.transport
            .as_ref()
            .and_then(|t| t.http_upgrade.as_ref())
            .is_some(),
        "httpupgrade transport lost in roundtrip: {out}"
    );
}

#[test]
fn vless_reality_roundtrip() {
    // 回归：reality 节点的 flow/pbk/sid 必须在 parse→serialize 后保留——
    // 丢失 pbk/sid 后节点无法建立连接（线上事故：聚合订阅 3/4 节点不可用）。
    let uri = "vless://11111111-2222-3333-4444-555555555555@1.2.3.4:443?encryption=none&security=reality&flow=xtls-rprx-vision&sni=www.as979.net&fp=chrome&pbk=sAm7vnX_zAavonzGYm4C0BRsl8lwwdPyvEivwLoQNQ8&sid=6ba85179e30d4fc2#US-01";
    let n = parse_vless(uri).unwrap();
    assert_eq!(n.flow.as_deref(), Some("xtls-rprx-vision"), "flow 必须解析");
    assert_eq!(
        n.pbk.as_deref(),
        Some("sAm7vnX_zAavonzGYm4C0BRsl8lwwdPyvEivwLoQNQ8"),
        "reality 公钥必须解析"
    );
    assert_eq!(
        n.sid.as_deref(),
        Some("6ba85179e30d4fc2"),
        "shortId 必须解析"
    );
    let out = serialize_vless(&n).unwrap();
    assert!(
        out.contains("security=reality"),
        "security 保持 reality: {out}"
    );
    assert!(out.contains("flow=xtls-rprx-vision"), "flow 保留: {out}");
    assert!(
        out.contains("pbk=sAm7vnX_zAavonzGYm4C0BRsl8lwwdPyvEivwLoQNQ8"),
        "pbk 保留: {out}"
    );
    assert!(out.contains("sid=6ba85179e30d4fc2"), "sid 保留: {out}");
    let n2 = parse_vless(&out).unwrap();
    assert_eq!(n2.flow, n.flow);
    assert_eq!(n2.pbk, n.pbk);
    assert_eq!(n2.sid, n.sid);
    assert_eq!(n2.server, n.server);
    assert_eq!(n2.port, n.port);
}

#[test]
fn vless_invalid() {
    assert!(parse_vless("vless://").is_err());
    assert!(parse_vless("vless://bad@host").is_err());
}
