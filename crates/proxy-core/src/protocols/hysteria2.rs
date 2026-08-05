// crates/proxy-core/src/protocols/hysteria2.rs
use crate::error::{ParseError, SerializeError};
use crate::model::{Protocol, ProxyNode, TlsSettings};
use crate::uri::{parse_host_port, percent_decode, split_authority};
use urlencoding::encode;

pub fn is_hysteria2(uri: &str) -> bool {
    uri.starts_with("hysteria2://") || uri.starts_with("hy2://")
}

pub fn parse_hysteria2(uri: &str) -> Result<ProxyNode, ParseError> {
    let rest = uri
        .strip_prefix("hysteria2://")
        .or_else(|| uri.strip_prefix("hy2://"))
        .ok_or(ParseError::UnsupportedProtocol)?;
    let (auth_query, fragment) = match rest.find('#') {
        Some(i) => (&rest[..i], Some(&rest[i + 1..])),
        None => (rest, None),
    };
    let (userinfo, host_query) = split_authority(auth_query);
    let (hostpart, query) = match host_query.find('?') {
        Some(i) => (&host_query[..i], Some(&host_query[i + 1..])),
        None => (host_query, None),
    };
    let (server, port) = parse_host_port(hostpart)?;
    let password = if userinfo.is_empty() { None } else { Some(percent_decode(userinfo)?) };

    let mut sni = None;
    let mut alpn = Vec::new();
    let mut insecure = false;

    if let Some(q) = query {
        for kv in q.split('&') {
            let Some((k, v)) = kv.split_once('=') else { continue };
            let v = percent_decode(v).unwrap_or_default();
            match k {
                "sni" => sni = Some(v),
                "alpn" => alpn = vec![v],
                "insecure" => insecure = v == "1" || v == "true",
                _ => {}
            }
        }
    }

    Ok(ProxyNode {
        name: fragment.map(|f| f.to_string()).unwrap_or_default(),
        kind: Protocol::Hysteria2,
        server,
        port,
        password,
        tls: Some(TlsSettings {
            enabled: true,
            sni,
            alpn,
            insecure,
            fingerprint: None,
        }),
        ..Default::default()
    })
}

pub fn serialize_hysteria2(node: &ProxyNode) -> Result<String, SerializeError> {
    if node.kind != Protocol::Hysteria2 {
        return Err(SerializeError::UnsupportedProtocol(node.kind.as_str()));
    }
    let mut out = format!(
        "hysteria2://{}@{}:{}",
        encode(node.password.as_deref().unwrap_or_default()),
        node.server,
        node.port
    );
    if let Some(t) = &node.tls {
        if let Some(s) = &t.sni {
            out.push_str(&format!("?sni={}", encode(s)));
        }
        if let Some(a) = t.alpn.first() {
            out.push_str(&format!("&alpn={}", encode(a)));
        }
        if t.insecure {
            out.push_str("&insecure=1");
        }
    }
    if !node.name.is_empty() {
        out.push('#');
        out.push_str(&node.name);
    }
    Ok(out)
}
