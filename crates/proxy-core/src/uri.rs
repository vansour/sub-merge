// crates/proxy-core/src/uri.rs
use crate::error::ParseError;

pub fn decode_base64_url(s: &str) -> Result<Vec<u8>, ParseError> {
    use base64::Engine;
    let mut t = s.trim().to_string();
    // 转成标准 alphabet 并补 padding
    t = t.replace('-', "+").replace('_', "/");
    // 若输入已带尾部 padding，先去除再按需补齐
    t = t.trim_end_matches('=').to_string();
    match t.len() % 4 {
        2 => t.push_str("=="),
        3 => t.push_str("="),
        1 => return Err(ParseError::InvalidBase64(s.to_string())),
        _ => {}
    }
    base64::engine::general_purpose::STANDARD
        .decode(t.as_bytes())
        .map_err(|_| ParseError::InvalidBase64(s.to_string()))
}

pub fn decode_base64_url_string(s: &str) -> Result<String, ParseError> {
    let bytes = decode_base64_url(s)?;
    String::from_utf8(bytes).map_err(|_| ParseError::InvalidBase64(s.to_string()))
}

pub fn percent_decode(s: &str) -> Result<String, ParseError> {
    percent_encoding::percent_decode_str(s)
        .decode_utf8()
        .map(|c| c.to_string())
        .map_err(|_| ParseError::InvalidUri(s.to_string()))
}

/// 在最后一个 '@' 处拆分。返回 (userinfo, hostpart)。
pub fn split_authority(auth: &str) -> (&str, &str) {
    match auth.rfind('@') {
        Some(idx) => (&auth[..idx], &auth[idx + 1..]),
        None => ("", auth),
    }
}

pub fn parse_host_port(s: &str) -> Result<(String, u16), ParseError> {
    // 处理 IPv6: [addr]:port
    let (host, port_str) = if let Some(rest) = s.strip_prefix('[') {
        let close = rest.find(']').ok_or_else(|| ParseError::InvalidUri(s.to_string()))?;
        let host = rest[..close].to_string();
        let after = &rest[close + 1..];
        let port_str = after
            .strip_prefix(':')
            .ok_or_else(|| ParseError::InvalidUri(s.to_string()))?;
        (host, port_str)
    } else {
        let idx = s.rfind(':').ok_or_else(|| ParseError::InvalidUri(s.to_string()))?;
        (s[..idx].to_string(), &s[idx + 1..])
    };
    let port: u16 = port_str
        .parse()
        .map_err(|_| ParseError::InvalidPort)?;
    Ok((host, port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_url_safe_padding_optional() {
        let b64 = "Y2hhY2hhMjAtaWV0Zi1wb2x5MTMwNTpwYXNz";
        let out = decode_base64_url_string(b64).unwrap();
        assert_eq!(out, "chacha20-ietf-poly1305:pass");
    }

    #[test]
    fn base64_standard_works() {
        let b64 = "Y2hhY2hhMjAtaWV0Zi1wb2x5MTMwNTpwYXNz=";
        assert_eq!(decode_base64_url_string(b64).unwrap(), "chacha20-ietf-poly1305:pass");
    }

    #[test]
    fn base64_invalid_returns_error() {
        assert!(decode_base64_url_string("!!!not-base64!!!").is_err());
    }

    #[test]
    fn split_authority_last_at() {
        let (user, host) = split_authority("user@name@1.2.3.4");
        assert_eq!(user, "user@name");
        assert_eq!(host, "1.2.3.4");
    }

    #[test]
    fn parse_host_port_ok() {
        let (host, port) = parse_host_port("example.com:443").unwrap();
        assert_eq!(host, "example.com");
        assert_eq!(port, 443);
    }

    #[test]
    fn parse_host_port_ipv6() {
        let (host, port) = parse_host_port("[2001:db8::1]:8388").unwrap();
        assert_eq!(host, "2001:db8::1");
        assert_eq!(port, 8388);
    }

    #[test]
    fn parse_host_port_bad() {
        assert!(parse_host_port("no-port-here").is_err());
        assert!(parse_host_port("host:99999").is_err());
    }
}
