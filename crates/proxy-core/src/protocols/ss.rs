// crates/proxy-core/src/protocols/ss.rs
use crate::error::{ParseError, SerializeError};
use crate::model::{Crypto, ProxyNode};
use crate::uri::{decode_base64_url_string, parse_host_port, percent_decode, split_authority};

pub fn is_ss(uri: &str) -> bool {
    uri.starts_with("ss://")
}

pub fn parse_ss(uri: &str) -> Result<ProxyNode, ParseError> {
    let rest = uri
        .strip_prefix("ss://")
        .ok_or(ParseError::UnsupportedProtocol)?;
    let (auth, fragment) = match rest.find('#') {
        Some(i) => (&rest[..i], Some(&rest[i + 1..])),
        None => (rest, None),
    };

    // 分离 query（plugin 参数，本版解析时忽略具体 plugin 但保留 tail）
    let (userinfo_and_host, _query) = match auth.find('?') {
        Some(i) => (&auth[..i], Some(&auth[i + 1..])),
        None => (auth, None),
    };

    let (userinfo, hostport) = split_authority(userinfo_and_host);
    let (server, port) = parse_host_port(hostport.trim_end_matches('/'))?;

    // 判断 userinfo 是 base64 还是明文。明文特征：含 ':' 且第一个冒号前是已知加密名。
    let (crypto, password) = parse_userinfo(userinfo)?;

    let name = fragment.map(|f| f.to_string()).unwrap_or_default();

    Ok(ProxyNode {
        name,
        kind: crate::model::Protocol::Ss,
        server,
        port,
        crypto: Some(crypto),
        password: Some(password),
        ..Default::default()
    })
}

fn parse_userinfo(userinfo: &str) -> Result<(Crypto, String), ParseError> {
    // 尝试明文解析：method:password（method 不含 ':'）
    if let Some(idx) = userinfo.find(':') {
        let method = &userinfo[..idx];
        let rest = &userinfo[idx + 1..];
        // 明文 method 必须是合法加密名（含连字符或数字，不含 base64 特殊字符）
        if is_known_method(method) {
            let password = percent_decode(rest)?;
            return Ok((Crypto::from_str(method), password));
        }
    }
    // 否则按 base64 解析（legacy 格式）：base64(method:password)
    let decoded = decode_base64_url_string(userinfo)?;
    let idx = decoded
        .find(':')
        .ok_or_else(|| ParseError::InvalidUri(userinfo.to_string()))?;
    let method = &decoded[..idx];
    let password = decoded[idx + 1..].to_string();
    Ok((Crypto::from_str(method), password))
}

fn is_known_method(m: &str) -> bool {
    let m = m.to_ascii_lowercase();
    matches!(
        m.as_str(),
        "aes-256-gcm"
            | "aes-128-gcm"
            | "chacha20-ietf-poly1305"
            | "aes-128-cfb"
            | "aes-192-cfb"
            | "aes-256-cfb"
            | "chacha20-ietf"
            | "salsa20"
            | "rc4-md5"
            | "2022-blake3-aes-256-gcm"
            | "2022-blake3-aes-128-gcm"
            | "2022-blake3-chacha20-poly1305"
            | "none"
            | "plain"
    )
}

pub fn serialize_ss(node: &ProxyNode) -> Result<String, SerializeError> {
    if node.kind != crate::model::Protocol::Ss {
        return Err(SerializeError::UnsupportedProtocol(node.kind.as_str()));
    }
    let crypto = node
        .crypto
        .as_ref()
        .ok_or(SerializeError::MissingField("crypto"))?;
    let password = node
        .password
        .as_ref()
        .ok_or(SerializeError::MissingField("password"))?;
    let userinfo = format!("{}:{}", crypto.as_str(), password);
    let b64 = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD_NO_PAD,
        userinfo.as_bytes(),
    );
    let mut out = format!("ss://{}@{}:{}", b64, node.server, node.port);
    if !node.name.is_empty() {
        out.push('#');
        out.push_str(&node.name);
    }
    Ok(out)
}
