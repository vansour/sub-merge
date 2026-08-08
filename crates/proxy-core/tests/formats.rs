use proxy_core::formats::clash::{serialize_clash, serialize_clash_subscription};
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
fn clash_skips_unserializable_node() {
    // wireguard 节点可解析但缺 privateKey 序列化失败（proxy_to_clash 返回 Err，
    // if let Ok 跳过）；ss 节点正常。输出只含正常节点，不因坏节点失败。
    let nodes = vec![
        ss_node("h", "h", 8388),
        proxy_core::parser::parse_line(
            "wireguard://cHVibGljS2V5MTIz@1.2.3.4:443?publicKey=cHVibGljS2V5MTIz#WG",
        )
        .unwrap(),
    ];
    let out = serialize_clash(&nodes).unwrap();
    assert!(
        out.contains("name: h") && out.contains("port: 8388"),
        "正常节点保留"
    );
    assert!(!out.contains("WG"), "不可序列化节点被跳过");
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
fn serialize_dispatch() {
    let nodes = vec![ss_node("A", "1.2.3.4", 8388)];
    let clash = serialize_nodes(&nodes, OutputFormat::Clash).unwrap();
    assert!(clash.contains("proxies:"));
    let v2 = serialize_nodes(&nodes, OutputFormat::V2ray).unwrap();
    assert!(v2.contains("ss://") || v2.len() > 20);
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
}

#[test]
fn clash_subscription_default_template() {
    let tpl = "mixed-port: 7890\nallow-lan: false\nmode: rule\nlog-level: info\n";
    let out =
        serialize_clash_subscription(tpl, "home", "http://x/subscribe/home?format=v2ray").unwrap();
    assert!(out.contains("mixed-port: 7890"), "头部保留");
    assert!(out.contains("proxy-providers:"));
    assert!(out.contains("home:"));
    assert!(out.contains("url: http://x/subscribe/home?format=v2ray"));
    assert!(out.contains("proxy-groups:"));
    assert!(out.contains("use:"));
    assert!(out.contains("- home"));
    // 输出必须是合法 YAML 且 providers 恰好一个
    let v: serde_yaml_ng::Value = serde_yaml_ng::from_str(&out).unwrap();
    let prov = v["proxy-providers"].as_mapping().unwrap();
    assert_eq!(prov.len(), 1);
    assert!(prov.contains_key(serde_yaml_ng::Value::String("home".into())));
}

#[test]
fn clash_subscription_keeps_custom_sections() {
    let tpl = "mode: rule\ndns:\n  enable: true\n  nameserver:\n    - 1.1.1.1\nrules:\n  - DOMAIN-SUFFIX,google.com,🚀 节点选择\n";
    let out = serialize_clash_subscription(tpl, "home", "http://x/sub").unwrap();
    assert!(out.contains("dns:"));
    assert!(out.contains("enable: true"));
    assert!(out.contains("1.1.1.1"));
    assert!(out.contains("DOMAIN-SUFFIX,google.com"));
}

#[test]
fn clash_subscription_system_sections_override() {
    let tpl = "proxy-providers:\n  evil: {type: file, path: ./x}\nproxy-groups:\n  - name: evil\n";
    let out = serialize_clash_subscription(tpl, "home", "http://x/sub").unwrap();
    let v: serde_yaml_ng::Value = serde_yaml_ng::from_str(&out).unwrap();
    let prov = v["proxy-providers"].as_mapping().unwrap();
    assert_eq!(prov.len(), 1, "模板 providers 必须被系统段覆盖");
    assert!(prov.contains_key(serde_yaml_ng::Value::String("home".into())));
    assert!(!out.contains("evil"), "模板 providers/groups 不得残留");
}

#[test]
fn clash_subscription_invalid_template() {
    assert!(serialize_clash_subscription(": : :", "home", "http://x").is_err());
    assert!(
        serialize_clash_subscription("", "home", "http://x").is_ok(),
        "空模板视为空映射，合法"
    );
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
