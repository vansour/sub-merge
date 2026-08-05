// crates/proxy-core/src/protocols/wireguard.rs
use crate::error::{ParseError, SerializeError};
use crate::model::{Protocol, ProxyNode};
use crate::uri::{parse_host_port, percent_decode, split_authority};
use urlencoding::encode;

pub fn is_wireguard(uri: &str) -> bool {
    uri.starts_with("wireguard://")
}

pub fn parse_wireguard(uri: &str) -> Result<ProxyNode, ParseError> {
    let rest = uri.strip_prefix("wireguard://").ok_or(ParseError::UnsupportedProtocol)?;
    let (host_query, fragment) = match rest.find('#') {
        Some(i) => (&rest[..i], Some(&rest[i + 1..])),
        None => (rest, None),
    };
    let (auth_query, query) = match host_query.find('?') {
        Some(i) => (&host_query[..i], Some(&host_query[i + 1..])),
        None => (host_query, None),
    };
    // userinfo 部分为 base64url(publicKey)
    let (userinfo, hostpart) = split_authority(auth_query);
    let (server, port) = parse_host_port(hostpart)?;

    let mut public_key = if userinfo.is_empty() {
        None
    } else {
        Some(percent_decode(userinfo)?)
    };
    let mut private_key = None;
    // model 暂未提供 IP/网段字段，仅解析不存储
    let mut _ips = Vec::new();

    if let Some(q) = query {
        for kv in q.split('&') {
            let Some((k, v)) = kv.split_once('=') else { continue };
            let v = percent_decode(v).unwrap_or_default();
            match k {
                "publicKey" => public_key = Some(v),
                "privateKey" => private_key = Some(v),
                "ip" => _ips = v.split(',').map(|s| s.to_string()).collect(),
                _ => {}
            }
        }
    }
    if public_key.is_none() {
        return Err(ParseError::MissingField("publicKey"));
    }

    Ok(ProxyNode {
        name: fragment.map(|f| f.to_string()).unwrap_or_default(),
        kind: Protocol::Wireguard,
        server,
        port,
        uuid: private_key,
        password: public_key,
        ..Default::default()
    })
}

pub fn serialize_wireguard(node: &ProxyNode) -> Result<String, SerializeError> {
    if node.kind != Protocol::Wireguard {
        return Err(SerializeError::UnsupportedProtocol(node.kind.as_str()));
    }
    let pubk = node.password.as_ref().ok_or(SerializeError::MissingField("publicKey"))?;
    let privk = node.uuid.as_ref().ok_or(SerializeError::MissingField("privateKey"))?;
    let mut out = format!("wireguard://{}@{}:{}", encode(pubk), node.server, node.port);
    out.push_str(&format!("?publicKey={}", encode(pubk)));
    out.push_str(&format!("&privateKey={}", encode(privk)));
    out.push_str("&reserved=0,0,0&mtu=1420");
    out.push_str("&ip=10.0.0.1/24");
    if !node.name.is_empty() {
        out.push('#');
        out.push_str(&node.name);
    }
    Ok(out)
}
