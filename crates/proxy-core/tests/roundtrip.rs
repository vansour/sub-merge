use proxy_core::model::Protocol;
use proxy_core::protocols::hysteria::{parse_hysteria, serialize_hysteria};
use proxy_core::protocols::hysteria2::{parse_hysteria2, serialize_hysteria2};
use proxy_core::protocols::ss::{parse_ss, serialize_ss};
use proxy_core::protocols::ssr::{parse_ssr, serialize_ssr};
use proxy_core::protocols::trojan::{parse_trojan, serialize_trojan};
use proxy_core::protocols::tuic::{parse_tuic, serialize_tuic};
use proxy_core::protocols::vless::{parse_vless, serialize_vless};
use proxy_core::protocols::vmess::{parse_vmess, serialize_vmess};
use proxy_core::protocols::wireguard::{parse_wireguard, serialize_wireguard};

/// 对每个协议的 (解析, 序列化) 对做往返：解析→序列化→再解析，关键字段不变。
macro_rules! roundtrip {
    ($uri:expr, $parser:ident, $serializer:ident, $kind:expr) => {{
        let n1 = $parser($uri).unwrap();
        assert_eq!(n1.kind, $kind);
        let out = $serializer(&n1).unwrap();
        let n2 = $parser(&out).unwrap();
        assert_eq!(n2.server, n1.server, "server changed for {}", $uri);
        assert_eq!(n2.port, n1.port, "port changed for {}", $uri);
    }};
}

#[test]
fn all_protocols_roundtrip() {
    roundtrip!(
        "ss://YWVzLTI1Ni1nY206cGFzc3dvcmQ@example.com:8388#US-01",
        parse_ss,
        serialize_ss,
        Protocol::Ss
    );
    roundtrip!(
        "ssr://MS4yLjMuNDo4Mzg4OmF1dGhfYWVzMTI4X21kNTphZXMtMjU2LWNmYjpwbGFpbjpjR0Z6Y3c9PS8_cmVtYXJrcz1WVk10TURF",
        parse_ssr,
        serialize_ssr,
        Protocol::Ssr
    );
    roundtrip!(
        "vmess://eyJ2IjoiMiIsInBzIjoiU0ctMDEiLCJhZGQiOiIxLjIuMy40IiwicG9ydCI6IjQ0MyIsImlkIjoidXVpZC0xMTExLTExMTEiLCJhaWQiOiIwIiwibmV0Ijoid3MiLCJ0eXBlIjoibm9uZSIsImhvc3QiOiJjZG4uZXhhbXBsZS5jb20iLCJwYXRoIjoiL3dzIiwidGxzIjoidGxzIn0=",
        parse_vmess,
        serialize_vmess,
        Protocol::Vmess
    );
    roundtrip!(
        "vless://11111111-2222-3333-4444-555555555555@1.2.3.4:443?encryption=none&security=tls&sni=cdn.example.com&type=ws&path=%2Fws&fp=chrome#JP-01",
        parse_vless,
        serialize_vless,
        Protocol::Vless
    );
    roundtrip!(
        "trojan://pass%40word@1.2.3.4:443?security=tls&sni=example.com#KR-01",
        parse_trojan,
        serialize_trojan,
        Protocol::Trojan
    );
    roundtrip!(
        "hysteria://1.2.3.4:36712?protocol=udp&auth=secret123&upmbps=100&downmbps=100&obfs=xorsalsa20&sni=example.com&insecure=1&alpn=hysteria#JP-01",
        parse_hysteria,
        serialize_hysteria,
        Protocol::Hysteria
    );
    roundtrip!(
        "hysteria2://pass%40word@1.2.3.4:8443?sni=example.com&insecure=0#US-02",
        parse_hysteria2,
        serialize_hysteria2,
        Protocol::Hysteria2
    );
    roundtrip!(
        "tuic://11111111-2222-3333-4444-555555555555:pass%40word@1.2.3.4:443?congestion_control=bbr&udp_relay_mode=native&alpn=h3&sni=example.com&allow_insecure=1#TW-01",
        parse_tuic,
        serialize_tuic,
        Protocol::Tuic
    );
    roundtrip!(
        "wireguard://cHVibGljS2V5MTIz@1.2.3.4:443?publicKey=cHVibGljS2V5MTIz&privateKey=cHJpdmF0ZUtleTEyMw==&reserved=0,0,0&mtu=1420&ip=10.0.0.1%2F24#SG-01",
        parse_wireguard,
        serialize_wireguard,
        Protocol::Wireguard
    );
}

#[test]
fn full_pipeline_parse_merge_serialize() {
    use proxy_core::parser::parse_subscription_text;
    use proxy_core::serializer::{OutputFormat, serialize_nodes};

    let sub = "ss://YWVzLTI1Ni1nY206cGFzc3dvcmQ@example.com:8388#A\nvmess://eyJ2IjoiMiIsInBzIjoiQiIsImFkZCI6IjUuNi43LjgiLCJwb3J0IjoiNDQzIiwiaWQiOiJ1IiwiYWlkIjoiMCIsIm5ldCI6InRjcCIsInRscyI6Im5vbmUifQ==";
    let (nodes, skipped) = parse_subscription_text(sub, 1000);
    assert_eq!(nodes.len(), 2);
    assert_eq!(skipped, 0);

    let clash = serialize_nodes(&nodes, OutputFormat::Clash).unwrap();
    assert!(clash.contains("proxies:"));
    assert!(clash.contains("name: A"));
    assert!(clash.contains("name: B"));
}
