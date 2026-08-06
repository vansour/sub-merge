// crates/proxy-core/src/parser.rs
use crate::error::ParseError;
use crate::model::ProxyNode;
use crate::protocols::{
    http, hysteria, hysteria2, ss, ssr, socks5, trojan, tuic, vless, vmess, wireguard,
};
use crate::uri::decode_base64_url_string;

/// 按协议前缀分发到各协议 parser。
pub fn parse_line(line: &str) -> Result<ProxyNode, ParseError> {
    let l = line.trim();
    if l.is_empty() {
        return Err(ParseError::UnsupportedProtocol);
    }
    if ss::is_ss(l) {
        ss::parse_ss(l)
    } else if ssr::is_ssr(l) {
        ssr::parse_ssr(l)
    } else if vmess::is_vmess(l) {
        vmess::parse_vmess(l)
    } else if vless::is_vless(l) {
        vless::parse_vless(l)
    } else if trojan::is_trojan(l) {
        trojan::parse_trojan(l)
    } else if socks5::is_socks5(l) {
        socks5::parse_socks5(l)
    } else if http::is_http(l) {
        http::parse_http(l)
    } else if hysteria2::is_hysteria2(l) {
        hysteria2::parse_hysteria2(l)
    } else if hysteria::is_hysteria(l) {
        hysteria::parse_hysteria(l)
    } else if tuic::is_tuic(l) {
        tuic::parse_tuic(l)
    } else if wireguard::is_wireguard(l) {
        wireguard::parse_wireguard(l)
    } else {
        Err(ParseError::UnsupportedProtocol)
    }
}

/// 解析 base64 订阅：先解码再逐行解析。
pub fn parse_v2ray_subscription(text: &str, max_nodes: usize) -> (Vec<ProxyNode>, usize) {
    // 若整个 body 是 base64，则先解码
    let body = decode_base64_url_string(text).unwrap_or_else(|_| text.to_string());
    parse_lines(&body, max_nodes)
}

fn parse_lines(text: &str, max_nodes: usize) -> (Vec<ProxyNode>, usize) {
    let mut nodes = Vec::new();
    let mut skipped = 0usize;
    for line in text.lines() {
        if nodes.len() >= max_nodes {
            break;
        }
        match parse_line(line) {
            Ok(n) => nodes.push(n),
            Err(_) => skipped += 1,
        }
    }
    (nodes, skipped)
}

/// 自动识别订阅文本：Clash YAML（proxies: 标志）→ base64 解码 → 明文逐行。
pub fn parse_subscription_text(text: &str, max_nodes: usize) -> (Vec<ProxyNode>, usize) {
    // Clash YAML 订阅：任一行 trim 后以 "proxies:" 开头则按 YAML 解析（在 base64 尝试之前，
    // 避免 YAML 文本被误当 base64；解析失败回退逐行）
    if text.lines().any(|l| l.trim_start().starts_with("proxies:")) {
        return match parse_clash_yaml(text) {
            Ok(mut nodes) => {
                nodes.truncate(max_nodes);
                (nodes, 0)
            }
            Err(_) => parse_lines(text, max_nodes),
        };
    }
    // 若文本看起来是纯 base64（无协议前缀），尝试整体解码
    let trimmed = text.trim();
    let looks_base64 = !trimmed.contains("://") && trimmed.len() > 16;
    if looks_base64
        && let Ok(decoded) = decode_base64_url_string(trimmed)
        && (decoded.contains('\n') || decoded.contains("://"))
    {
        return parse_lines(&decoded, max_nodes);
    }
    parse_lines(text, max_nodes)
}

/// 解析 Clash YAML 的 proxies 段。
pub fn parse_clash_yaml(text: &str) -> Result<Vec<ProxyNode>, ParseError> {
    let doc: serde_yaml::Value = serde_yaml::from_str(text).map_err(|e| ParseError::InvalidUri(e.to_string()))?;
    let Some(proxies) = doc.get("proxies").and_then(|p| p.as_sequence()) else {
        return Ok(Vec::new());
    };
    let mut nodes = Vec::new();
    for p in proxies {
        if let Some(n) = clash_proxy_to_node(p) {
            nodes.push(n);
        }
    }
    Ok(nodes)
}

fn clash_proxy_to_node(p: &serde_yaml::Value) -> Option<ProxyNode> {
    let name = p.get("name")?.as_str()?.to_string();
    let ty = p.get("type")?.as_str()?.to_lowercase();
    let server = p.get("server")?.as_str()?.to_string();
    let port: u16 = p.get("port")?.as_u64()?.try_into().ok()?;

    let mut node = ProxyNode {
        name,
        kind: crate::model::Protocol::from_str(&ty)?,
        server,
        port,
        ..Default::default()
    };
    // 各类型共性字段
    if let Some(s) = p.get("password").and_then(|v| v.as_str()) {
        node.password = Some(s.to_string());
    }
    if let Some(s) = p.get("uuid").and_then(|v| v.as_str()) {
        node.uuid = Some(s.to_string());
    }
    if let Some(alter_id) = p.get("alterId").or_else(|| p.get("alter-id")).and_then(|v| v.as_u64()) {
        node.alter_id = alter_id.try_into().ok();
    }
    // 加密
    if let Some(c) = p.get("cipher").or_else(|| p.get("method")).and_then(|v| v.as_str())
        && c != "auto"
    {
        node.crypto = Some(crate::model::Crypto::from_str(c));
    }
    // TLS
    // trojan 协议按 Clash 语义始终启用 TLS（协议本身要求 TLS 承载）。
    let type_always_tls = ty == "trojan";
    let tls_on = type_always_tls
        || p.get("tls").and_then(|v| v.as_bool()).unwrap_or(false)
        || p.get("security").and_then(|v| v.as_str()).map(|s| s != "none").unwrap_or(false);
    if tls_on {
        node.tls = Some(crate::model::TlsSettings {
            enabled: true,
            sni: p.get("sni").and_then(|v| v.as_str()).map(|s| s.to_string())
                .or_else(|| p.get("servername").and_then(|v| v.as_str()).map(|s| s.to_string())),
            alpn: p.get("alpn").and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default(),
            insecure: p.get("skip-cert-verify").and_then(|v| v.as_bool()).unwrap_or(false),
            fingerprint: p.get("client-fingerprint").and_then(|v| v.as_str()).map(|s| s.to_string()),
        });
    }
    // 传输
    let net = p.get("network").and_then(|v| v.as_str()).unwrap_or("tcp");
    match net {
        "ws" | "websocket" => {
            let path = p.get("ws-opts").and_then(|o| o.get("path")).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let host = p.get("ws-opts").and_then(|o| o.get("headers")).and_then(|h| h.get("Host")).and_then(|v| v.as_str()).map(|s| s.to_string());
            node.transport = Some(crate::model::Transport {
                websocket: Some(crate::model::WebsocketConfig { path, host, headers: Default::default() }),
                ..Default::default()
            });
        }
        "grpc" => {
            let service = p.get("grpc-opts").and_then(|o| o.get("grpc-service-name")).and_then(|v| v.as_str()).unwrap_or("").to_string();
            node.transport = Some(crate::model::Transport {
                grpc: Some(crate::model::GrpcConfig { service_name: service }),
                ..Default::default()
            });
        }
        _ => {}
    }
    Some(node)
}
