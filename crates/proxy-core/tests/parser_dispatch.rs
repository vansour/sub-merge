use base64::Engine;
use proxy_core::model::Protocol;
use proxy_core::parser::{
    parse_clash_yaml, parse_line, parse_subscription_text, parse_v2ray_subscription,
};

// 对应 JSON: {"v":"2","ps":"SG-01","add":"1.2.3.4","port":"443","id":"uuid-1111-1111","aid":"0","net":"ws","type":"none","host":"cdn.example.com","path":"/ws","tls":"tls"}
const VMESS: &str = "vmess://eyJ2IjoiMiIsInBzIjoiU0ctMDEiLCJhZGQiOiIxLjIuMy40IiwicG9ydCI6IjQ0MyIsImlkIjoidXVpZC0xMTExLTExMTEiLCJhaWQiOiIwIiwibmV0Ijoid3MiLCJ0eXBlIjoibm9uZSIsImhvc3QiOiJjZG4uZXhhbXBsZS5jb20iLCJwYXRoIjoiL3dzIiwidGxzIjoidGxzIn0=";

#[test]
fn dispatch_by_prefix() {
    assert_eq!(
        parse_line("ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#T")
            .unwrap()
            .kind,
        Protocol::Ss
    );
    assert_eq!(parse_line(VMESS).unwrap().kind, Protocol::Vmess);
    assert!(parse_line("unknown://x").is_err());
}

#[test]
fn v2ray_subscription_base64() {
    // 多行 ss 链接拼成 base64
    let plain = "ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#A\nss://YWVzLTI1Ni1nY206cGFzcw@h:8389#B";
    let encoded = Engine::encode(&base64::engine::general_purpose::STANDARD, plain.as_bytes());
    let (nodes, _) = parse_v2ray_subscription(&encoded, 100);
    assert_eq!(nodes.len(), 2);
}

#[test]
fn subscription_plaintext_lines() {
    let text =
        "ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#A\nss://YWVzLTI1Ni1nY206cGFzcw@h:8389#B\nbad-line";
    let (nodes, skipped) = parse_subscription_text(text, 100);
    assert_eq!(nodes.len(), 2);
    assert_eq!(skipped, 1);
}

#[test]
fn subscription_max_nodes_limits() {
    let mut lines = String::new();
    for i in 0..10 {
        lines.push_str(&format!(
            "ss://YWVzLTI1Ni1nY206cGFzcw@h:{}#N{}\n",
            8000 + i,
            i
        ));
    }
    let (nodes, _) = parse_subscription_text(&lines, 5);
    assert_eq!(nodes.len(), 5);
}

#[test]
fn clash_yaml_parsing() {
    let yaml = r#"
proxies:
  - name: "JP-01"
    type: trojan
    server: 1.2.3.4
    port: 443
    password: pass123
    sni: example.com
  - name: "US-01"
    type: vmess
    server: 5.6.7.8
    port: 443
    uuid: 11111111-2222-3333-4444-555555555555
    alterId: 0
    cipher: auto
    tls: true
    network: ws
    ws-opts:
      path: /ws
      headers:
        Host: cdn.example.com
"#;
    let nodes = parse_clash_yaml(yaml).unwrap();
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].kind, Protocol::Trojan);
    assert_eq!(nodes[0].name, "JP-01");
    assert_eq!(nodes[0].password.as_deref(), Some("pass123"));
    assert_eq!(nodes[1].kind, Protocol::Vmess);
    let ws = nodes[1]
        .transport
        .as_ref()
        .and_then(|t| t.websocket.as_ref())
        .unwrap();
    assert_eq!(ws.path, "/ws");
}

#[test]
fn subscription_auto_detects_clash_yaml() {
    let yaml = "proxies:\n  - name: \"JP-01\"\n    type: trojan\n    server: 1.2.3.4\n    port: 443\n    password: pass123\n";
    let (nodes, skipped) = parse_subscription_text(yaml, 100);
    assert_eq!(nodes.len(), 1, "clash yaml source must parse");
    assert_eq!(nodes[0].name, "JP-01");
    assert_eq!(nodes[0].kind, Protocol::Trojan);
    assert_eq!(skipped, 0);
}

#[test]
fn subscription_oversized_line_skipped() {
    // 单行超过 1MB：跳过并计数，不解析。
    // 行长必须能被正常解析（明文 userinfo + 大密码），否则失败原因是解析错误而非长度限制。
    let huge = format!(
        "ss://aes-256-gcm:{}@1.2.3.4:8388#N",
        "p".repeat(1024 * 1024)
    );
    let (nodes, skipped) = parse_subscription_text(&huge, 100);
    assert_eq!(nodes.len(), 0);
    assert_eq!(skipped, 1);
}

#[test]
fn subscription_yaml_respects_max_nodes() {
    let mut yaml = String::from("proxies:\n");
    for i in 0..10 {
        yaml.push_str(&format!(
            "  - name: \"N{i}\"\n    type: ss\n    server: 1.2.3.4\n    port: {}\n    cipher: aes-256-gcm\n    password: pw\n",
            8000 + i
        ));
    }
    let (nodes, _) = parse_subscription_text(&yaml, 5);
    assert_eq!(nodes.len(), 5);
}

#[test]
fn clash_yaml_vless_reality_opts() {
    // 回归：Clash YAML 的 vless reality 节点（reality-opts + flow）必须解析出
    // pbk/sid/flow——与 vless URI 路径对称，丢失 pbk/sid 后节点无法连接。
    let yaml = r#"
proxies:
  - name: "US-01"
    type: vless
    server: 1.2.3.4
    port: 443
    uuid: 11111111-2222-3333-4444-555555555555
    network: tcp
    tls: true
    servername: www.as979.net
    client-fingerprint: chrome
    reality-opts:
      public-key: sAm7vnX_zAavonzGYm4C0BRsl8lwwdPyvEivwLoQNQ8
      short-id: 6ba85179e30d4fc2
    flow: xtls-rprx-vision
"#;
    let nodes = parse_clash_yaml(yaml).unwrap();
    assert_eq!(nodes.len(), 1);
    let n = &nodes[0];
    assert_eq!(n.kind, Protocol::Vless);
    assert_eq!(
        n.pbk.as_deref(),
        Some("sAm7vnX_zAavonzGYm4C0BRsl8lwwdPyvEivwLoQNQ8")
    );
    assert_eq!(n.sid.as_deref(), Some("6ba85179e30d4fc2"));
    assert_eq!(n.flow.as_deref(), Some("xtls-rprx-vision"));
    let tls = n.tls.as_ref().expect("reality 节点启用 TLS 承载");
    assert!(tls.enabled);
}

#[test]
fn clash_yaml_trojan_sni_without_tls_field() {
    // Clash trojan 条目通常只有顶层 sni，无显式 tls 字段。
    let yaml = r#"
proxies:
  - name: "JP-01"
    type: trojan
    server: 1.2.3.4
    port: 443
    password: pass123
    sni: example.com
"#;
    let nodes = parse_clash_yaml(yaml).unwrap();
    assert_eq!(nodes.len(), 1);
    let n = &nodes[0];
    assert_eq!(n.kind, Protocol::Trojan);
    let tls = n.tls.as_ref().expect("trojan should have TLS settings");
    assert!(tls.enabled, "trojan requires TLS");
    assert_eq!(tls.sni.as_deref(), Some("example.com"));
}
