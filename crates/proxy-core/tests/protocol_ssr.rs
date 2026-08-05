use proxy_core::model::{Crypto, Protocol, ProxyNode};
use proxy_core::protocols::ssr::{parse_ssr, serialize_ssr};

// SSR 链接整体 base64url（不带 padding），解码后明文为:
//   1.2.3.4:8388:auth_aes128_md5:aes-256-cfb:plain:cGFzcw==/?remarks=VVMtMDE
// 其中第 6 段 cGFzcw== 是 "pass" 的 base64，query 中 remarks=VVMtMDE 是 "US-01" 的 base64。
const SSR: &str = "ssr://MS4yLjMuNDo4Mzg4OmF1dGhfYWVzMTI4X21kNTphZXMtMjU2LWNmYjpwbGFpbjpjR0Z6Y3c9PS8_cmVtYXJrcz1WVk10TURF";

#[test]
fn parse_ssr_basic() {
    let n = parse_ssr(SSR).unwrap();
    assert_eq!(n.kind, Protocol::Ssr);
    assert_eq!(n.server, "1.2.3.4");
    assert_eq!(n.port, 8388);
    assert_eq!(n.crypto, Some(Crypto::Aes256Cfb));
    assert_eq!(n.password.as_deref(), Some("pass"));
    assert_eq!(n.name, "US-01");
}

#[test]
fn parse_ssr_invalid() {
    assert!(parse_ssr("ssr://").is_err());
    assert!(parse_ssr("not-ssr").is_err());
}

#[test]
fn serialize_roundtrip() {
    let n = parse_ssr(SSR).unwrap();
    let out = serialize_ssr(&n).unwrap();
    assert!(out.starts_with("ssr://"));
    let n2 = parse_ssr(&out).unwrap();
    assert_eq!(n2.server, n.server);
    assert_eq!(n2.port, n.port);
    assert_eq!(n2.password, n.password);
}

#[test]
fn serialize_roundtrip_password_with_slash_in_standard_b64() {
    // 回归：password "aa?" 的 STANDARD base64 是 "YWE/"，含 '/'。
    // 若内部 password 用 STANDARD 编码，parse_ssr_decoded 的 find('/') 会在 base64 中截断，丢掉 '?'。
    // 内部改用 base64url 后应完整往返。
    let n = ProxyNode {
        name: "US-01".into(),
        kind: Protocol::Ssr,
        server: "1.2.3.4".into(),
        port: 8388,
        crypto: Some(Crypto::Aes256Cfb),
        password: Some("aa?".into()),
        ..Default::default()
    };
    let out = serialize_ssr(&n).unwrap();
    assert!(out.starts_with("ssr://"));
    let n2 = parse_ssr(&out).unwrap();
    assert_eq!(n2.server, n.server);
    assert_eq!(n2.port, n.port);
    assert_eq!(n2.password.as_deref(), Some("aa?"));
    assert_eq!(n2.name, n.name);
}
