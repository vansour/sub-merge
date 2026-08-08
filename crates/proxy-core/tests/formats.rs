// v2ray 输出格式测试（clash 输出已移除，见 2026-08-08 去 clash 变更）
use proxy_core::formats::v2ray::{serialize_v2ray, serialize_v2ray_plain};
use proxy_core::model::{Crypto, Protocol, ProxyNode};

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
fn v2ray_subscription_uri_lines() {
    let nodes = vec![ss_node("A", "1.2.3.4", 8388)];
    let out = serialize_v2ray(&nodes).unwrap();
    // 是 base64，解码后含 ss://
    let decoded = proxy_core::uri::decode_base64_url_string(&out).unwrap();
    assert!(decoded.contains("ss://"));
    assert!(decoded.contains("1.2.3.4:8388"));
}

#[test]
fn v2ray_plain_outputs_uri_lines() {
    let nodes = vec![
        ss_node("A", "1.2.3.4", 8388),
        trojan_node("B", "5.6.7.8", 443),
    ];
    let out = serialize_v2ray_plain(&nodes).unwrap();
    assert!(out.contains("ss://"), "纯 URI 行，非 base64");
    assert!(out.contains("trojan://"));
    assert!(out.contains('\n'), "每行一个节点");
}

#[test]
fn v2ray_plain_empty() {
    assert_eq!(serialize_v2ray_plain(&[]).unwrap(), "");
}

#[test]
fn v2ray_empty_nodes_ok() {
    assert!(serialize_v2ray(&[]).is_ok());
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
