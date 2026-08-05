// crates/proxy-core/src/protocols/socks5.rs
use crate::error::{ParseError, SerializeError};
use crate::model::{Protocol, ProxyNode};
use crate::uri::{parse_host_port, percent_decode, split_authority};

pub fn is_socks5(uri: &str) -> bool {
    uri.starts_with("socks5://")
}

pub fn parse_socks5(uri: &str) -> Result<ProxyNode, ParseError> {
    let rest = uri.strip_prefix("socks5://").ok_or(ParseError::UnsupportedProtocol)?;
    let (auth, fragment) = match rest.find('#') {
        Some(i) => (&rest[..i], Some(&rest[i + 1..])),
        None => (rest, None),
    };
    let (userinfo, hostport) = split_authority(auth);
    let (server, port) = parse_host_port(hostport)?;
    let (_user, password) = if userinfo.is_empty() {
        (None, None)
    } else {
        let (u, p) = split_userpass(userinfo)?;
        (Some(u), Some(p))
    };
    Ok(ProxyNode {
        name: fragment.map(|f| f.to_string()).unwrap_or_default(),
        kind: Protocol::Socks5,
        server,
        port,
        password,
        ..Default::default()
    })
}

fn split_userpass(s: &str) -> Result<(String, String), ParseError> {
    let (u, p) = s.split_once(':').ok_or_else(|| ParseError::InvalidUri(s.to_string()))?;
    Ok((percent_decode(u)?, percent_decode(p)?))
}

pub fn serialize_socks5(node: &ProxyNode) -> Result<String, SerializeError> {
    if node.kind != Protocol::Socks5 {
        return Err(SerializeError::UnsupportedProtocol(node.kind.as_str()));
    }
    let mut out = String::from("socks5://");
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
