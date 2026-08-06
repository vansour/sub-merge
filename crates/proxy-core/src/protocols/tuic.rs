// crates/proxy-core/src/protocols/tuic.rs
use crate::error::{ParseError, SerializeError};
use crate::model::{Protocol, ProxyNode, TlsSettings};
use crate::uri::{parse_host_port, percent_decode, split_authority};
use urlencoding::encode;

pub fn is_tuic(uri: &str) -> bool {
    uri.starts_with("tuic://")
}

pub fn parse_tuic(uri: &str) -> Result<ProxyNode, ParseError> {
    let rest = uri
        .strip_prefix("tuic://")
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

    // userinfo = uuid:password
    let (uuid, password) = userinfo
        .split_once(':')
        .ok_or(ParseError::MissingField("uuid"))?;
    let password = percent_decode(password)?;

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
                "sni" => sni = Some(v),
                "alpn" => alpn = v.split(',').map(|s| s.to_string()).collect(),
                "allow_insecure" | "insecure" => insecure = v == "1" || v == "true",
                _ => {}
            }
        }
    }

    Ok(ProxyNode {
        name: fragment.map(|f| f.to_string()).unwrap_or_default(),
        kind: Protocol::Tuic,
        server,
        port,
        uuid: Some(uuid.to_string()),
        password: Some(password),
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

pub fn serialize_tuic(node: &ProxyNode) -> Result<String, SerializeError> {
    if node.kind != Protocol::Tuic {
        return Err(SerializeError::UnsupportedProtocol(node.kind.as_str()));
    }
    let uuid = node.uuid.as_deref().unwrap_or_default();
    let password = encode(node.password.as_deref().unwrap_or_default());
    let mut out = format!("tuic://{}:{}@{}:{}", uuid, password, node.server, node.port);
    out.push_str("?congestion_control=bbr&udp_relay_mode=native");
    if let Some(t) = &node.tls {
        if let Some(s) = &t.sni {
            out.push_str(&format!("&sni={}", encode(s)));
        }
        if !t.alpn.is_empty() {
            out.push_str(&format!("&alpn={}", encode(&t.alpn.join(","))));
        }
        if t.insecure {
            out.push_str("&allow_insecure=1");
        }
    }
    if !node.name.is_empty() {
        out.push('#');
        out.push_str(&node.name);
    }
    Ok(out)
}
