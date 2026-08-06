// crates/proxy-core/src/uri.rs
use crate::error::ParseError;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

pub fn decode_base64_url(s: &str) -> Result<Vec<u8>, ParseError> {
    use base64::Engine;
    // 解码膨胀防护：超过 4MB 的 base64 输入直接拒绝
    const MAX_BASE64_LEN: usize = 4 * 1024 * 1024;
    if s.len() > MAX_BASE64_LEN {
        return Err(ParseError::InvalidBase64(s.to_string()));
    }
    let t = s.trim();
    // 尾部 padding 之前的内容长度（等价于 trim_end_matches('=')，'=' 为 ASCII，
    // 其起始字节位置必为字符边界，无 UTF-8 截断风险）
    let end = t.bytes().rposition(|b| b != b'=').map_or(0, |i| i + 1);
    // 单遍转换：URL-safe 字母表 → standard 字母表（'-'→'+'、'_'→'/'），
    // 直接写入输出缓冲，消除中间字符串分配
    let mut buf = Vec::with_capacity(end + 2);
    buf.extend(t.bytes().take(end).map(|b| match b {
        b'-' => b'+',
        b'_' => b'/',
        b => b,
    }));
    // 按需补齐 padding
    match buf.len() % 4 {
        2 => buf.extend_from_slice(b"=="),
        3 => buf.push(b'='),
        1 => return Err(ParseError::InvalidBase64(s.to_string())),
        _ => {}
    }
    base64::engine::general_purpose::STANDARD
        .decode(&buf)
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

/// 与 urlencoding::encode 语义等价：保留 RFC3986 unreserved（字母数字与 -_.~），
/// 其余字符（含空格、UTF-8 多字节）逐字节 percent-encode（空格 → %20 而非 +）。
const URLENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

pub fn urlencode(s: &str) -> String {
    utf8_percent_encode(s, URLENCODE_SET).to_string()
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
        let close = rest
            .find(']')
            .ok_or_else(|| ParseError::InvalidUri(s.to_string()))?;
        let host = rest[..close].to_string();
        let after = &rest[close + 1..];
        let port_str = after
            .strip_prefix(':')
            .ok_or_else(|| ParseError::InvalidUri(s.to_string()))?;
        (host, port_str)
    } else {
        let idx = s
            .rfind(':')
            .ok_or_else(|| ParseError::InvalidUri(s.to_string()))?;
        (s[..idx].to_string(), &s[idx + 1..])
    };
    let port: u16 = port_str.parse().map_err(|_| ParseError::InvalidPort)?;
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
        assert_eq!(
            decode_base64_url_string(b64).unwrap(),
            "chacha20-ietf-poly1305:pass"
        );
    }

    #[test]
    fn base64_invalid_returns_error() {
        assert!(decode_base64_url_string("!!!not-base64!!!").is_err());
    }

    #[test]
    fn base64_oversized_rejected() {
        // 长度 %4==0（无 padding 问题）且超 4MB 上限：长度限制必须拒绝
        let big = "A".repeat(4 * 1024 * 1024 + 4);
        assert!(decode_base64_url_string(&big).is_err());
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
