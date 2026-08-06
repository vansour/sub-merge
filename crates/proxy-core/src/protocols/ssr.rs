// crates/proxy-core/src/protocols/ssr.rs
use crate::error::{ParseError, SerializeError};
use crate::model::{Crypto, ProxyNode};
use crate::uri::decode_base64_url_string;

pub fn is_ssr(uri: &str) -> bool {
    uri.starts_with("ssr://")
}

pub fn parse_ssr(uri: &str) -> Result<ProxyNode, ParseError> {
    let payload = uri
        .strip_prefix("ssr://")
        .ok_or(ParseError::UnsupportedProtocol)?;
    // payload 整体是 base64url（不带 padding）。
    let decoded = decode_base64_url_string(payload)?;
    parse_ssr_decoded(&decoded)
}

fn parse_ssr_decoded(decoded: &str) -> Result<ProxyNode, ParseError> {
    // 拆出主段与 query。主段形如 server:port:protocol:method:obfs:base64(password)
    // query 分隔符是 "/?"（规范形态）或 "?"。不能用裸 '/' 切分：密码段的 standard
    // base64 可能含 '/'（6-bit 组值 63），会拦腰截断密码。'?' 不在 base64 字母表内，
    // 用 find("/?") 优先、find('?') 兜底；分隔符前的 '/' 从主段去掉。
    let sep = decoded.find("/?").or_else(|| decoded.find('?'));
    let (main, query) = match sep {
        Some(i) => {
            let end = if decoded.as_bytes()[i - 1] == b'/' {
                i - 1
            } else {
                i
            };
            (&decoded[..end], &decoded[i..])
        }
        None => (decoded, ""),
    };
    let parts: Vec<&str> = main.split(':').collect();
    if parts.len() < 6 {
        return Err(ParseError::InvalidUri(decoded.to_string()));
    }
    let server = parts[0].to_string();
    let port: u16 = parts[1].parse().map_err(|_| ParseError::InvalidPort)?;
    let _protocol = parts[2]; // SSR 协议层（auth_aes128_md5 等），当前中间模型不单独建模
    let method = parts[3];
    let _obfs = parts[4];
    let password_b64 = parts[5];
    let password = decode_base64_url_string(password_b64)?;

    // 解析 query 中的 remarks/group 等。query 形如 "/?remarks=...&group=..."
    let mut name = String::new();
    for kv in query.split('&') {
        // 去掉开头的 "/?" 前缀
        let kv = kv.trim_start_matches('/').trim_start_matches('?');
        if let Some((k, v)) = kv.split_once('=')
            && k == "remarks"
        {
            name = decode_base64_url_string(v).unwrap_or_default();
        }
    }

    Ok(ProxyNode {
        name,
        kind: crate::model::Protocol::Ssr,
        server,
        port,
        crypto: Some(Crypto::from_str(method)),
        password: Some(password),
        ..Default::default()
    })
}

pub fn serialize_ssr(node: &ProxyNode) -> Result<String, SerializeError> {
    if node.kind != crate::model::Protocol::Ssr {
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
    // 内部 password / remarks 用 base64url（无 padding），避免 STANDARD 字母表产生 '/'
    // 干扰 parse_ssr_decoded 中的 query 分隔符 find('/')。解析端 decode_base64_url 两种字母表都兼容。
    let pass_b64 = base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        password.as_bytes(),
    );
    let remarks_b64 = base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        node.name.as_bytes(),
    );
    let plain = format!(
        "{}:{}:{}:{}:{}:{}/?remarks={}",
        node.server,
        node.port,
        "auth_aes128_md5",
        crypto.as_str(),
        "plain",
        pass_b64,
        remarks_b64
    );
    // 整体 base64url（去 padding）
    let b64 = base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        plain.as_bytes(),
    );
    Ok(format!("ssr://{}", b64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_ssr_detects_prefix() {
        assert!(is_ssr("ssr://abc"));
        assert!(!is_ssr("ss://abc"));
        assert!(!is_ssr("vmess://abc"));
    }

    #[test]
    fn parse_handles_obfsparam_no_value() {
        // 明文: 1.2.3.4:8388:auth_aes128_md5:aes-256-cfb:plain:cGFzcw==/?obfsparam=&id=1&remarks=VVMtMDE
        let uri = "ssr://MS4yLjMuNDo4Mzg4OmF1dGhfYWVzMTI4X21kNTphZXMtMjU2LWNmYjpwbGFpbjpjR0Z6Y3c9PS8_b2Jmc3BhcmFtPSZpZD0xJnJlbWFya3M9VlZNdE1ERQ";
        let n = parse_ssr(uri).unwrap();
        assert_eq!(n.server, "1.2.3.4");
        assert_eq!(n.port, 8388);
        assert_eq!(n.password.as_deref(), Some("pass"));
        assert_eq!(n.name, "US-01");
    }

    #[test]
    fn parse_password_base64_containing_slash() {
        // 回归：密码的 standard base64 含 '/' 时不能被误切为 query 分隔符。
        // 明文主段: 1.2.3.4:8388:auth_aes128_md5:aes-256-cfb:plain:cGFzcy8=  （密码 "pass/"，b64 含 '/'）
        // 无 query（find("/?")/find('?') 均不命中）→ 整体不切分。
        let plain = "1.2.3.4:8388:auth_aes128_md5:aes-256-cfb:plain:cGFzcy8=";
        let uri = format!(
            "ssr://{}",
            base64::Engine::encode(
                &base64::engine::general_purpose::URL_SAFE_NO_PAD,
                plain.as_bytes()
            )
        );
        let n = parse_ssr(&uri).unwrap();
        assert_eq!(n.server, "1.2.3.4");
        assert_eq!(n.port, 8388);
        assert_eq!(n.password.as_deref(), Some("pass/"));

        // 带 "/?remarks=" query：分隔符切分正确，密码保留完整
        let plain = "1.2.3.4:8388:auth_aes128_md5:aes-256-cfb:plain:cGFzcy8=/?remarks=MQ";
        let uri = format!(
            "ssr://{}",
            base64::Engine::encode(
                &base64::engine::general_purpose::URL_SAFE_NO_PAD,
                plain.as_bytes()
            )
        );
        let n = parse_ssr(&uri).unwrap();
        assert_eq!(n.password.as_deref(), Some("pass/"));
        assert_eq!(n.name, "1");
    }

    #[test]
    fn serialize_rejects_wrong_protocol() {
        let n = ProxyNode::new("x".into(), crate::model::Protocol::Ss, "h".into(), 1);
        assert!(serialize_ssr(&n).is_err());
    }

    #[test]
    fn parse_rejects_bad_port_and_short() {
        // 明文: 1.2.3.4:notaport:auth:plain:plain:cGFzcw==
        let uri = "ssr://MS4yLjMuNDpub3RhcG9ydDphdXRoOnBsYWluOnBsYWluOmNHRnpjdz09";
        assert!(parse_ssr(uri).is_err());
        // 明文: 1.2.3.4:8388  （不足 6 段）
        let short = "ssr://MS4yLjMuNDo4Mzg4";
        assert!(parse_ssr(short).is_err());
    }
}
