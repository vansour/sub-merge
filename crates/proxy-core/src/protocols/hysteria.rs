// crates/proxy-core/src/protocols/hysteria.rs
use crate::error::{ParseError, SerializeError};
use crate::model::{Protocol, ProxyNode, TlsSettings};
use crate::uri::{parse_host_port, percent_decode, urlencode};

pub fn is_hysteria(uri: &str) -> bool {
    uri.starts_with("hysteria://")
}

pub fn parse_hysteria(uri: &str) -> Result<ProxyNode, ParseError> {
    let rest = uri
        .strip_prefix("hysteria://")
        .ok_or(ParseError::UnsupportedProtocol)?;
    let (host_query, fragment) = match rest.find('#') {
        Some(i) => (&rest[..i], Some(&rest[i + 1..])),
        None => (rest, None),
    };
    let (hostpart, query) = match host_query.find('?') {
        Some(i) => (&host_query[..i], Some(&host_query[i + 1..])),
        None => (host_query, None),
    };
    let (server, port) = parse_host_port(hostpart)?;

    let mut auth = None;
    let mut sni = None;
    let mut alpn = Vec::new();
    let mut insecure = false;

    if let Some(q) = query {
        for kv in q.split('&') {
            let Some((k, v)) = kv.split_once('=') else {
                continue;
            };
            let v = percent_decode(v).unwrap_or_default();
            match k {
                "auth" => auth = Some(v),
                "sni" => sni = Some(v),
                "alpn" => alpn = vec![v],
                "insecure" => insecure = v == "1" || v == "true",
                _ => {}
            }
        }
    }

    Ok(ProxyNode {
        name: fragment.map(|f| f.to_string()).unwrap_or_default(),
        kind: Protocol::Hysteria,
        server,
        port,
        password: auth,
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

pub fn serialize_hysteria(node: &ProxyNode) -> Result<String, SerializeError> {
    if node.kind != Protocol::Hysteria {
        return Err(SerializeError::UnsupportedProtocol(node.kind.as_str()));
    }
    let auth = node.password.as_deref().unwrap_or_default();
    let mut out = format!(
        "hysteria://{}:{}?protocol=udp&auth={}",
        node.server,
        node.port,
        urlencode(auth)
    );
    if let Some(t) = &node.tls {
        if let Some(s) = &t.sni {
            out.push_str(&format!("&sni={}", urlencode(s)));
        }
        if let Some(a) = t.alpn.first() {
            out.push_str(&format!("&alpn={}", urlencode(a)));
        }
        if t.insecure {
            out.push_str("&insecure=1");
        }
    }
    out.push_str("&upmbps=100&downmbps=100&obfs=xorsalsa20");
    if !node.name.is_empty() {
        out.push('#');
        out.push_str(&node.name);
    }
    Ok(out)
}
