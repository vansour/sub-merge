// crates/proxy-core/src/protocols/http.rs
use crate::error::{ParseError, SerializeError};
use crate::model::{Protocol, ProxyNode};
use crate::uri::{parse_host_port, percent_decode, split_authority};

pub fn is_http(uri: &str) -> bool {
    uri.starts_with("http://") && !uri.starts_with("http://example") // 防御误判（此处仅按前缀即可）
}

pub fn parse_http(uri: &str) -> Result<ProxyNode, ParseError> {
    let rest = uri.strip_prefix("http://").ok_or(ParseError::UnsupportedProtocol)?;
    let (auth, fragment) = match rest.find('#') {
        Some(i) => (&rest[..i], Some(&rest[i + 1..])),
        None => (rest, None),
    };
    let (userinfo, hostport) = split_authority(auth);
    let (server, port) = parse_host_port(hostport)?;
    let password = if userinfo.is_empty() {
        None
    } else {
        let (_, p) = userinfo.split_once(':').ok_or_else(|| ParseError::InvalidUri(userinfo.to_string()))?;
        Some(percent_decode(p)?)
    };
    Ok(ProxyNode {
        name: fragment.map(|f| f.to_string()).unwrap_or_default(),
        kind: Protocol::Http,
        server,
        port,
        password,
        ..Default::default()
    })
}

pub fn serialize_http(node: &ProxyNode) -> Result<String, SerializeError> {
    if node.kind != Protocol::Http {
        return Err(SerializeError::UnsupportedProtocol(node.kind.as_str()));
    }
    let mut out = String::from("http://");
    if let Some(p) = &node.password {
        out.push_str(&node.server);
        out.push(':');
        out.push_str(p);
        out.push('@');
    }
    out.push_str(&node.server);
    out.push(':');
    out.push_str(&node.port.to_string());
    if !node.name.is_empty() {
        out.push('#');
        out.push_str(&node.name);
    }
    Ok(out)
}
