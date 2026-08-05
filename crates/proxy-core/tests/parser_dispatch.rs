use base64::Engine;
use proxy_core::model::Protocol;
use proxy_core::parser::{parse_clash_yaml, parse_line, parse_subscription_text, parse_v2ray_subscription};

// 对应 JSON: {"v":"2","ps":"SG-01","add":"1.2.3.4","port":"443","id":"uuid-1111-1111","aid":"0","net":"ws","type":"none","host":"cdn.example.com","path":"/ws","tls":"tls"}
const VMESS: &str = "vmess://eyJ2IjoiMiIsInBzIjoiU0ctMDEiLCJhZGQiOiIxLjIuMy40IiwicG9ydCI6IjQ0MyIsImlkIjoidXVpZC0xMTExLTExMTEiLCJhaWQiOiIwIiwibmV0Ijoid3MiLCJ0eXBlIjoibm9uZSIsImhvc3QiOiJjZG4uZXhhbXBsZS5jb20iLCJwYXRoIjoiL3dzIiwidGxzIjoidGxzIn0=";

#[test]
fn dispatch_by_prefix() {
    assert_eq!(parse_line("ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#T").unwrap().kind, Protocol::Ss);
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
    let text = "ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#A\nss://YWVzLTI1Ni1nY206cGFzcw@h:8389#B\nbad-line";
    let (nodes, skipped) = parse_subscription_text(text, 100);
    assert_eq!(nodes.len(), 2);
    assert_eq!(skipped, 1);
}

#[test]
fn subscription_max_nodes_limits() {
    let mut lines = String::new();
    for i in 0..10 {
        lines.push_str(&format!("ss://YWVzLTI1Ni1nY206cGFzcw@h:{}#N{}\n", 8000 + i, i));
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
    let ws = nodes[1].transport.as_ref().and_then(|t| t.websocket.as_ref()).unwrap();
    assert_eq!(ws.path, "/ws");
}
