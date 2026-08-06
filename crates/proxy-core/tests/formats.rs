use proxy_core::formats::clash::serialize_clash;
use proxy_core::formats::singbox::serialize_singbox;
use proxy_core::formats::v2ray::serialize_v2ray;
use proxy_core::model::{Crypto, Protocol, ProxyNode};
use proxy_core::serializer::{OutputFormat, serialize_nodes};

fn ss_node(name: &str, server: &str, port: u16) -> ProxyNode {
    ProxyNode {
        name: name.into(),
        kind: Protocol::Ss,
        server: server.into(),
        port,
        crypto: Some(Crypto::Aes256Gcm),
        password: Some("pass".into()),
        ..Default::default()
    }
}

fn trojan_node(name: &str, server: &str, port: u16) -> ProxyNode {
    ProxyNode {
        name: name.into(),
        kind: Protocol::Trojan,
        server: server.into(),
        port,
        password: Some("pw".into()),
        tls: Some(proxy_core::model::TlsSettings {
            enabled: true,
            sni: Some(server.into()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn clash_yaml_has_proxies_and_groups() {
    let nodes = vec![
        ss_node("A", "1.2.3.4", 8388),
        trojan_node("B", "5.6.7.8", 443),
    ];
    let out = serialize_clash(&nodes).unwrap();
    assert!(out.contains("proxies:"));
    assert!(out.contains("proxy-groups:"));
    assert!(out.contains("name: A"));
    assert!(out.contains("type: ss"));
    assert!(out.contains("type: trojan"));
}

#[test]
fn v2ray_subscription_uri_lines() {
    let nodes = vec![ss_node("A", "1.2.3.4", 8388)];
    let out = serialize_v2ray(&nodes).unwrap();
    // 是 base64，解码后含 ss://
    let decoded = proxy_core::uri::decode_base64_url_string(&out).unwrap();
    assert!(decoded.contains("ss://"));
    assert!(decoded.contains("1.2.3.4:8388"));
}

#[test]
fn singbox_json_outbounds() {
    let nodes = vec![
        ss_node("A", "1.2.3.4", 8388),
        trojan_node("B", "5.6.7.8", 443),
    ];
    let out = serialize_singbox(&nodes).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let outbounds = v["outbounds"].as_array().unwrap();
    assert_eq!(outbounds.len(), 2);
    assert_eq!(outbounds[0]["type"], "shadowsocks");
    assert_eq!(outbounds[1]["type"], "trojan");
}

#[test]
fn serialize_dispatch() {
    let nodes = vec![ss_node("A", "1.2.3.4", 8388)];
    let clash = serialize_nodes(&nodes, OutputFormat::Clash).unwrap();
    assert!(clash.contains("proxies:"));
    let v2 = serialize_nodes(&nodes, OutputFormat::V2ray).unwrap();
    assert!(v2.contains("ss://") || v2.len() > 20);
    let sb = serialize_nodes(&nodes, OutputFormat::Singbox).unwrap();
    assert!(sb.contains("outbounds"));
}

#[test]
fn urlencode_equivalent_to_old_semantics() {
    // 与 urlencoding::encode 语义等价：保留 RFC3986 unreserved，其余逐字节转义，
    // 空格 → %20（非 +）
    assert_eq!(proxy_core::uri::urlencode("a b/c~"), "a%20b%2Fc~");
    assert_eq!(proxy_core::uri::urlencode("ABC-._~"), "ABC-._~");
    assert_eq!(proxy_core::uri::urlencode("日本"), "%E6%97%A5%E6%9C%AC");
    assert_eq!(proxy_core::uri::urlencode("a+b"), "a%2Bb");
    assert_eq!(proxy_core::uri::urlencode(""), "");
}

#[test]
fn empty_nodes_ok() {
    assert!(serialize_clash(&[]).is_ok());
    assert!(serialize_singbox(&[]).is_ok());
}

#[test]
fn clash_yaml_quotes_hostile_names() {
    use proxy_core::model::{Crypto, Protocol, ProxyNode};
    for name in [
        "!secret",
        "*alias",
        "a|b",
        "col:on",
        "a\"b",
        "{x}",
        "p@ss",
        "日本 东京",
    ] {
        let node = ProxyNode {
            name: name.into(),
            kind: Protocol::Ss,
            server: "1.2.3.4".into(),
            port: 8388,
            crypto: Some(Crypto::Aes256Gcm),
            password: Some("pw".into()),
            ..Default::default()
        };
        let out = serialize_clash(&[node]).unwrap();
        let v: serde_yaml_ng::Value = serde_yaml_ng::from_str(&out)
            .unwrap_or_else(|e| panic!("output must be valid yaml for {name:?}: {e}\n{out}"));
        assert_eq!(
            v["proxies"][0]["name"].as_str().unwrap(),
            name,
            "name must roundtrip for {name:?}"
        );
    }
}
