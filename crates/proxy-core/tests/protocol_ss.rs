use proxy_core::model::{Crypto, Protocol};
use proxy_core::protocols::ss::{parse_ss, serialize_ss};

const LEGACY: &str = "ss://YWVzLTI1Ni1nY206cGFzc3dvcmQ@example.com:8388#US-01";
const SIP002: &str = "ss://2022-blake3-aes-256-gcm:c3VwZXJzZWNyZXQ@example.com:8388/#Example";

#[test]
fn parse_legacy_userinfo() {
    let n = parse_ss(LEGACY).unwrap();
    assert_eq!(n.kind, Protocol::Ss);
    assert_eq!(n.server, "example.com");
    assert_eq!(n.port, 8388);
    assert_eq!(n.crypto, Some(Crypto::Aes256Gcm));
    assert_eq!(n.password.as_deref(), Some("password"));
    assert_eq!(n.name, "US-01");
}

#[test]
fn parse_sip002_plaintext() {
    let n = parse_ss(SIP002).unwrap();
    assert_eq!(n.kind, Protocol::Ss);
    assert_eq!(n.crypto, Some(Crypto::Plain));
    // password 是 "supersecret" 的 base64（SIP002 示例把 password 做 base64 仍是合法字符串，这里用原样）
    assert_eq!(n.password.as_deref(), Some("c3VwZXJzZWNyZXQ"));
    assert_eq!(n.name, "Example");
}

#[test]
fn parse_percent_encoded_password() {
    // 明文 userinfo 中的 password 需 percent-decode
    let uri = "ss://aes-256-gcm:pass%40word@example.com:8388#T";
    let n = parse_ss(uri).unwrap();
    assert_eq!(n.password.as_deref(), Some("pass@word"));
}

#[test]
fn parse_unknown_crypto_preserved() {
    let uri = "ss://Y3VzdG9tLWNpcGhlcjpwYXNz@example.com:8388"; // custom-cipher:pass
    let n = parse_ss(uri).unwrap();
    assert_eq!(n.crypto, Some(Crypto::Raw("custom-cipher".into())));
}

#[test]
fn serialize_roundtrip_basic() {
    let n = parse_ss(LEGACY).unwrap();
    let out = serialize_ss(&n).unwrap();
    assert!(out.starts_with("ss://"));
    let n2 = parse_ss(&out).unwrap();
    assert_eq!(n2.server, n.server);
    assert_eq!(n2.port, n.port);
    assert_eq!(n2.crypto, n.crypto);
    assert_eq!(n2.password, n.password);
}

#[test]
fn invalid_inputs() {
    assert!(parse_ss("ss://").is_err());
    assert!(parse_ss("not-ss").is_err());
    assert!(parse_ss("ss://aGVsbG8@host:notaport").is_err());
}
