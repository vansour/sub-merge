// crates/proxy-core/src/formats/clash.rs
use crate::error::SerializeError;
use crate::model::{Protocol, ProxyNode};

pub fn serialize_clash(nodes: &[ProxyNode]) -> Result<String, SerializeError> {
    let mut out = String::from("mixed-port: 7890\nallow-lan: false\nmode: rule\nlog-level: info\n\n");
    out.push_str("proxies:\n");
    for n in nodes {
        out.push_str(&proxy_to_clash(n)?);
    }
    if !nodes.is_empty() {
        out.push('\n');
        out.push_str("proxy-groups:\n");
        out.push_str("  - name: \"🚀 节点选择\"\n    type: select\n    proxies:\n");
        for n in nodes {
            out.push_str(&format!("      - {}\n", clash_yaml_str(&n.name)));
        }
        out.push_str("      - DIRECT\n");
        out.push_str("  - name: \"♻️ 自动选择\"\n    type: url-test\n    url: http://www.gstatic.com/generate_204\n    interval: 300\n    proxies:\n");
        for n in nodes {
            out.push_str(&format!("      - {}\n", clash_yaml_str(&n.name)));
        }
        out.push_str("\nrules:\n  - MATCH,🚀 节点选择\n");
    }
    Ok(out)
}

fn clash_yaml_str(s: &str) -> String {
    if s.chars().any(|c| c.is_whitespace() || c == ':' || c == '#' || c == ',' || c == '[' || c == ']') {
        format!("\"{}\"", s.replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

fn proxy_to_clash(n: &ProxyNode) -> Result<String, SerializeError> {
    let mut s = format!("  - name: {}\n    type: {}\n", clash_yaml_str(&n.name), clash_type(&n.kind));
    s.push_str(&format!("    server: {}\n    port: {}\n", n.server, n.port));
    match &n.kind {
        Protocol::Ss => {
            let c = n.crypto.as_ref().ok_or(SerializeError::MissingField("crypto"))?;
            let p = n.password.as_ref().ok_or(SerializeError::MissingField("password"))?;
            s.push_str(&format!("    cipher: {}\n    password: {}\n", clash_yaml_str(c.as_str()), clash_yaml_str(p)));
        }
        Protocol::Ssr => {
            let c = n.crypto.as_ref().ok_or(SerializeError::MissingField("crypto"))?;
            let p = n.password.as_ref().ok_or(SerializeError::MissingField("password"))?;
            s.push_str(&format!("    cipher: {}\n    password: {}\n", clash_yaml_str(c.as_str()), clash_yaml_str(p)));
        }
        Protocol::Vmess => {
            let u = n.uuid.as_ref().ok_or(SerializeError::MissingField("uuid"))?;
            s.push_str(&format!("    uuid: {}\n    alterId: {}\n    cipher: auto\n", clash_yaml_str(u), n.alter_id.unwrap_or(0)));
        }
        Protocol::Vless => {
            let u = n.uuid.as_ref().ok_or(SerializeError::MissingField("uuid"))?;
            s.push_str(&format!("    uuid: {}\n", clash_yaml_str(u)));
        }
        Protocol::Trojan => {
            let p = n.password.as_ref().ok_or(SerializeError::MissingField("password"))?;
            s.push_str(&format!("    password: {}\n", clash_yaml_str(p)));
        }
        Protocol::Hysteria | Protocol::Hysteria2 | Protocol::Tuic => {
            if let Some(p) = &n.password {
                s.push_str(&format!("    password: {}\n", clash_yaml_str(p)));
            }
            if let Some(u) = &n.uuid {
                s.push_str(&format!("    uuid: {}\n", clash_yaml_str(u)));
            }
        }
        Protocol::Socks5 => {
            if let Some(p) = &n.password {
                s.push_str(&format!("    username: {}\n    password: {}\n", clash_yaml_str(&n.server), clash_yaml_str(p)));
            }
        }
        Protocol::Http => {
            if let Some(p) = &n.password {
                s.push_str(&format!("    username: {}\n    password: {}\n", clash_yaml_str(&n.server), clash_yaml_str(p)));
            }
        }
        Protocol::Wireguard => {
            let pubk = n.password.as_ref().ok_or(SerializeError::MissingField("publicKey"))?;
            let privk = n.uuid.as_ref().ok_or(SerializeError::MissingField("privateKey"))?;
            s.push_str(&format!("    public-key: {}\n    private-key: {}\n", clash_yaml_str(pubk), clash_yaml_str(privk)));
        }
    }
    if let Some(t) = &n.tls {
        if t.enabled {
            s.push_str("    tls: true\n");
        }
        if let Some(sni) = &t.sni {
            s.push_str(&format!("    sni: {}\n", clash_yaml_str(sni)));
        }
        if !t.alpn.is_empty() {
            s.push_str(&format!("    alpn:\n      - {}\n", t.alpn.iter().map(|a| a.to_string()).collect::<Vec<_>>().join("\n      - ")));
        }
        if t.insecure {
            s.push_str("    skip-cert-verify: true\n");
        }
        if let Some(fp) = &t.fingerprint {
            s.push_str(&format!("    client-fingerprint: {}\n", clash_yaml_str(fp)));
        }
    }
    if let Some(tr) = &n.transport {
        if let Some(ws) = &tr.websocket {
            s.push_str("    network: ws\n    ws-opts:\n");
            s.push_str(&format!("      path: {}\n", clash_yaml_str(&ws.path)));
            if let Some(h) = &ws.host {
                s.push_str(&format!("      headers:\n        Host: {}\n", clash_yaml_str(h)));
            }
        } else if let Some(g) = &tr.grpc {
            s.push_str("    network: grpc\n    grpc-opts:\n");
            s.push_str(&format!("      grpc-service-name: {}\n", clash_yaml_str(&g.service_name)));
        } else if let Some(h) = &tr.http_upgrade {
            s.push_str("    network: http\n    http-opts:\n");
            s.push_str(&format!("      path: {}\n", clash_yaml_str(&h.path)));
        }
    }
    Ok(s)
}

fn clash_type(k: &Protocol) -> &'static str {
    match k {
        Protocol::Ss => "ss",
        Protocol::Ssr => "ssr",
        Protocol::Socks5 => "socks5",
        Protocol::Http => "http",
        Protocol::Vmess => "vmess",
        Protocol::Vless => "vless",
        Protocol::Trojan => "trojan",
        Protocol::Hysteria => "hysteria",
        Protocol::Hysteria2 => "hysteria2",
        Protocol::Tuic => "tuic",
        Protocol::Wireguard => "wireguard",
    }
}
