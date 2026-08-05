use proxy_core::model::Protocol;
use proxy_core::protocols::http::{parse_http, serialize_http};
use proxy_core::protocols::socks5::{parse_socks5, serialize_socks5};

#[test]
fn socks5_no_auth() {
    let n = parse_socks5("socks5://1.2.3.4:1080#HK-01").unwrap();
    assert_eq!(n.kind, Protocol::Socks5);
    assert_eq!(n.server, "1.2.3.4");
    assert_eq!(n.port, 1080);
    assert_eq!(n.name, "HK-01");
    assert_eq!(n.password, None);
}

#[test]
fn socks5_with_auth() {
    let n = parse_socks5("socks5://user:pass@1.2.3.4:1080#T").unwrap();
    assert_eq!(n.server, "1.2.3.4");
    assert_eq!(n.password.as_deref(), Some("pass"));
}

#[test]
fn socks5_percent_decoded_auth() {
    let n = parse_socks5("socks5://user:p%40ss@1.2.3.4:1080#T").unwrap();
    assert_eq!(n.password.as_deref(), Some("p@ss"));
}

#[test]
fn socks5_roundtrip() {
    let n = parse_socks5("socks5://user:pass@1.2.3.4:1080#T").unwrap();
    let out = serialize_socks5(&n).unwrap();
    assert!(out.starts_with("socks5://"));
    let n2 = parse_socks5(&out).unwrap();
    assert_eq!(n2.password, n.password);
    assert_eq!(n2.port, n.port);
}

#[test]
fn http_basic() {
    let n = parse_http("http://1.2.3.4:8080#JP").unwrap();
    assert_eq!(n.kind, Protocol::Http);
    assert_eq!(n.port, 8080);
    assert_eq!(n.name, "JP");
}

#[test]
fn http_roundtrip_with_auth() {
    let n = parse_http("http://u:pw@1.2.3.4:8080#JP").unwrap();
    let out = serialize_http(&n).unwrap();
    assert!(out.starts_with("http://"));
    let n2 = parse_http(&out).unwrap();
    assert_eq!(n2.password, n.password);
}

#[test]
fn invalid_legacy() {
    assert!(parse_socks5("socks5://host:abc").is_err());
    assert!(parse_http("http://").is_err());
}
