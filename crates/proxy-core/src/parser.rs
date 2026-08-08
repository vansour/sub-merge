// crates/proxy-core/src/parser.rs
use crate::error::ParseError;
use crate::model::ProxyNode;
use crate::protocols::{
    http, hysteria, hysteria2, socks5, ss, ssr, trojan, tuic, vless, vmess, wireguard,
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
        // 恶意超大行截断（>1MB）：跳过并计数
        if line.len() > 1024 * 1024 {
            skipped += 1;
            continue;
        }
        match parse_line(line) {
            Ok(n) => nodes.push(n),
            Err(_) => skipped += 1,
        }
    }
    (nodes, skipped)
}

/// 自动识别订阅文本：base64 解码 → 明文逐行。
pub fn parse_subscription_text(text: &str, max_nodes: usize) -> (Vec<ProxyNode>, usize) {
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
