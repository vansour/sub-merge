# Plan A: proxy-core 核心库 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 构建 `proxy-core` 纯 Rust 库：11 种协议的解析/序列化 + 3 种输出格式 + 往返测试。这是整个 sub-merge 的核心资产，无 IO、可单测、可复用。

**Architecture:** 单一中间模型 `ProxyNode` 覆盖所有协议，每种协议一个 Parser（解析 URI → ProxyNode）+ 一个 Serializer（ProxyNode → URI），输出格式（Clash/V2Ray/Sing-box）是独立的 Serializer 模块。所有解析器对不可信输入防御式处理（返回 Result，未知协议跳过）。

**Tech Stack:** Rust 1.97+, serde/serde_yaml, base64, urlencoding, percent-encoding, uuid（仅测试）

## Global Constraints

- Rust edition 2024，rust-version 1.97+
- workspace 根：`/root/github/sub-merge`，crate 路径：`crates/proxy-core`
- `ProxyNode` 为唯一中间模型，定义在 `crates/proxy-core/src/model.rs`
- 解析器签名统一：`pub fn parse(uri: &str) -> Result<ProxyNode, ParseError>`
- 序列化签名统一：`pub fn serialize(node: &ProxyNode) -> String`
- 所有协议解析/序列化均为纯函数，无 IO、无 panics
- 未知协议/非法输入返回 `Err(ParseError)`，不 panic
- 每个协议 Task 以 TDD 完成：先写失败测试 → 实现 → 通过 → commit
- 测试不依赖真实网络，全部本地样本

---

## 文件结构总览

```
crates/proxy-core/
├── Cargo.toml
├── src/
│   ├── lib.rs              # crate 根，re-export
│   ├── model.rs            # ProxyNode, Protocol, Crypto, Transport, TlsSettings
│   ├── error.rs            # ParseError, SerializeError
│   ├── uri.rs              # 通用 URI 工具（base64url 解码、percent-decode）
│   ├── parser.rs           # 按前缀分发到各协议 parser
│   ├── serializer.rs       # 分发：格式序列化入口
│   ├── protocols/
│   │   ├── mod.rs
│   │   ├── ss.rs           # ss:// 解析+序列化（SIP002）
│   │   ├── ssr.rs          # ssr:// 解析+序列化
│   │   ├── socks5.rs       # socks5:// 解析+序列化
│   │   ├── http.rs         # http:// 解析+序列化
│   │   ├── vmess.rs        # vmess:// 解析+序列化（v2rayN 新格式 + 老 JSON 格式）
│   │   ├── vless.rs        # vless:// 解析+序列化
│   │   ├── trojan.rs       # trojan:// 解析+序列化
│   │   ├── hysteria.rs     # hysteria:// 解析+序列化（hysteria1）
│   │   ├── hysteria2.rs    # hysteria2:// 解析+序列化
│   │   ├── tuic.rs         # tuic:// 解析+序列化
│   │   └── wireguard.rs    # wireguard:// 解析+序列化
│   └── formats/
│       ├── mod.rs
│       ├── v2ray_sub.rs    # base64 订阅文本解析（读入）
│       ├── clash.rs        # Clash YAML 序列化（写出）
│       ├── v2ray.rs        # V2Ray base64 序列化（写出）
│       └── singbox.rs      # Sing-box JSON 序列化（写出）
└── tests/
    ├── roundtrip.rs        # 全协议往返测试
    └── fixtures.rs         # 共享测试样本
```

---

### Task 1: Workspace 脚手架 + proxy-core crate + 中间模型

**Files:**
- Create: `/root/github/sub-merge/Cargo.toml`（workspace）
- Create: `/root/github/sub-merge/crates/proxy-core/Cargo.toml`
- Create: `/root/github/sub-merge/crates/proxy-core/src/lib.rs`
- Create: `/root/github/sub-merge/crates/proxy-core/src/model.rs`
- Create: `/root/github/sub-merge/crates/proxy-core/src/error.rs`

**Interfaces:**
- Consumes: 无
- Produces:
  - `enum Protocol { Ss, Ssr, Socks5, Http, Vmess, Vless, Trojan, Hysteria, Hysteria2, Tuic, Wireguard }`
  - `struct ProxyNode { name, kind, server, port, crypto, password, uuid, alter_id, tls, transport }`
  - `enum Crypto { Aes256Gcm, Aes128Gcm, Chacha20IetfPoly1305, Aes128Cfb, ... }`（枚举 + 兼容任意字符串的 fallback）
  - `struct TlsSettings { enabled: bool, sni: Option<String>, alpn: Vec<String>, insecure: bool, fingerprint: Option<String> }`
  - `struct Transport { websocket: Option<WebsocketConfig>, grpc: Option<GrpcConfig>, http_upgrade: bool, ... }`
  - `enum ParseError { UnsupportedProtocol, InvalidUri, InvalidBase64, InvalidPort, MissingField(&'static str), ... }` 实现 `Display` + `Error`
  - `enum SerializeError { UnsupportedProtocol, MissingField(&'static str) }`

- [ ] **Step 1: 创建 workspace 根 Cargo.toml**

```toml
[workspace]
resolver = "2"
members = ["crates/proxy-core"]

[workspace.package]
edition = "2024"
rust-version = "1.97"
```

- [ ] **Step 2: 创建 proxy-core Cargo.toml**

```toml
[package]
name = "proxy-core"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_yaml = "0.9"
base64 = "0.22"
urlencoding = "2"
percent-encoding = "2"
thiserror = "2"
```

- [ ] **Step 3: 写中间模型测试（model.rs 的行为契约）**

测试文件 `crates/proxy-core/src/model.rs` 内嵌 `#[cfg(test)]`：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_display_names() {
        assert_eq!(Protocol::Vmess.as_str(), "vmess");
        assert_eq!(Protocol::Hysteria2.as_str(), "hysteria2");
        assert_eq!(Protocol::Wireguard.as_str(), "wireguard");
    }

    #[test]
    fn node_constructs_with_defaults() {
        let n = ProxyNode::new("test".into(), Protocol::Ss, "1.2.3.4".into(), 8388);
        assert_eq!(n.name, "test");
        assert_eq!(n.kind, Protocol::Ss);
        assert_eq!(n.server, "1.2.3.4");
        assert_eq!(n.port, 8388);
        assert!(n.tls.is_none());
        assert!(n.transport.is_none());
    }
}
```

- [ ] **Step 4: 运行测试，确认编译失败（缺模型）**

Run: `cd /root/github/sub-merge && cargo test -p proxy-core`
Expected: FAIL — `error[E0425]: cannot find value Protocol` 等

- [ ] **Step 5: 实现 model.rs**

```rust
// crates/proxy-core/src/model.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Ss,
    Ssr,
    Socks5,
    Http,
    Vmess,
    Vless,
    Trojan,
    Hysteria,
    Hysteria2,
    Tuic,
    Wireguard,
}

impl Protocol {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ss => "ss",
            Self::Ssr => "ssr",
            Self::Socks5 => "socks5",
            Self::Http => "http",
            Self::Vmess => "vmess",
            Self::Vless => "vless",
            Self::Trojan => "trojan",
            Self::Hysteria => "hysteria",
            Self::Hysteria2 => "hysteria2",
            Self::Tuic => "tuic",
            Self::Wireguard => "wireguard",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "ss" => Some(Self::Ss),
            "ssr" => Some(Self::Ssr),
            "socks5" => Some(Self::Socks5),
            "socks" => Some(Self::Socks5),
            "http" => Some(Self::Http),
            "https" => Some(Self::Http),
            "vmess" => Some(Self::Vmess),
            "vless" => Some(Self::Vless),
            "trojan" => Some(Self::Trojan),
            "hysteria" | "hy1" => Some(Self::Hysteria),
            "hysteria2" | "hy2" => Some(Self::Hysteria2),
            "tuic" => Some(Self::Tuic),
            "wireguard" => Some(Self::Wireguard),
            _ => None,
        }
    }
}

/// 加密方式。常见项为枚举，未知值走 raw 保底（不丢信息）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Crypto {
    Aes256Gcm,
    Aes128Gcm,
    Chacha20IetfPoly1305,
    Aes128Cfb,
    Aes192Cfb,
    Aes256Cfb,
    Chacha20Ietf,
    Salsa20,
    Rc4Md5,
    // 保持明文（AEAD-2022 等长密钥）
    Plain,
    /// 未知/自定义加密方式，原样保留
    Raw(String),
}

impl Crypto {
    pub fn from_str(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "aes-256-gcm" => Self::Aes256Gcm,
            "aes-128-gcm" => Self::Aes128Gcm,
            "chacha20-ietf-poly1305" => Self::Chacha20IetfPoly1305,
            "aes-128-cfb" => Self::Aes128Cfb,
            "aes-192-cfb" => Self::Aes192Cfb,
            "aes-256-cfb" => Self::Aes256Cfb,
            "chacha20-ietf" => Self::Chacha20Ietf,
            "salsa20" => Self::Salsa20,
            "rc4-md5" => Self::Rc4Md5,
            "2022-blake3-aes-256-gcm" | "none" | "plain" => Self::Plain,
            other => Self::Raw(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Aes256Gcm => "aes-256-gcm",
            Self::Aes128Gcm => "aes-128-gcm",
            Self::Chacha20IetfPoly1305 => "chacha20-ietf-poly1305",
            Self::Aes128Cfb => "aes-128-cfb",
            Self::Aes192Cfb => "aes-192-cfb",
            Self::Aes256Cfb => "aes-256-cfb",
            Self::Chacha20Ietf => "chacha20-ietf",
            Self::Salsa20 => "salsa20",
            Self::Rc4Md5 => "rc4-md5",
            Self::Plain => "plain",
            Self::Raw(s) => s,
        }
    }
}

/// 传输层配置（vmess/vless 的 ws/grpc/httpupgrade 等）
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transport {
    pub websocket: Option<WebsocketConfig>,
    pub grpc: Option<GrpcConfig>,
    pub http_upgrade: Option<HttpUpgradeConfig>,
    pub shadow_tls: Option<ShadowTlsConfig>, // shadowtls 作为传输扩展
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebsocketConfig {
    pub path: String,
    pub host: Option<String>,
    pub headers: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrpcConfig {
    pub service_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpUpgradeConfig {
    pub path: String,
    pub host: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShadowTlsConfig {
    pub server: String,
    pub port: u16,
    pub password: String,
    pub sni: Option<String>,
}

/// TLS 配置
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TlsSettings {
    pub enabled: bool,
    pub sni: Option<String>,
    pub alpn: Vec<String>,
    pub insecure: bool,
    pub fingerprint: Option<String>,
}

/// 中间模型：所有协议统一表示
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProxyNode {
    pub name: String,
    pub kind: Protocol,
    pub server: String,
    pub port: u16,
    pub crypto: Option<Crypto>,
    pub password: Option<String>,
    pub uuid: Option<String>,
    pub alter_id: Option<u16>,
    pub tls: Option<TlsSettings>,
    pub transport: Option<Transport>,
}

impl ProxyNode {
    pub fn new(name: String, kind: Protocol, server: String, port: u16) -> Self {
        Self {
            name,
            kind,
            server,
            port,
            ..Default::default()
        }
    }
}
```

- [ ] **Step 6: 实现 error.rs**

```rust
// crates/proxy-core/src/error.rs
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("unsupported protocol")]
    UnsupportedProtocol,
    #[error("invalid URI: {0}")]
    InvalidUri(String),
    #[error("invalid base64: {0}")]
    InvalidBase64(String),
    #[error("invalid port")]
    InvalidPort,
    #[error("missing required field: {0}")]
    MissingField(&'static str),
    #[error("invalid value for {field}: {value}")]
    InvalidValue { field: &'static str, value: String },
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SerializeError {
    #[error("unsupported protocol for this format: {0}")]
    UnsupportedProtocol(&'static str),
    #[error("missing required field: {0}")]
    MissingField(&'static str),
}
```

- [ ] **Step 7: lib.rs 声明模块并 re-export**

```rust
// crates/proxy-core/src/lib.rs
pub mod error;
pub mod model;
pub mod uri;
pub mod parser;
pub mod serializer;
pub mod protocols;
pub mod formats;

pub use error::{ParseError, SerializeError};
pub use model::{Crypto, GrpcConfig, HttpUpgradeConfig, Protocol, ProxyNode, ShadowTlsConfig, TlsSettings, Transport, WebsocketConfig};
```

- [ ] **Step 8: 运行测试确认通过**

Run: `cargo test -p proxy-core`
Expected: PASS（2 个测试）

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "feat(proxy-core): workspace scaffolding, ProxyNode model, error types"
```

---

### Task 2: 通用 URI 工具（uri.rs）

**Files:**
- Create: `crates/proxy-core/src/uri.rs`
- Test: 同文件内 `#[cfg(test)]`

**Interfaces:**
- Consumes: `ParseError`
- Produces:
  - `pub fn decode_base64_url(s: &str) -> Result<Vec<u8>, ParseError>` — 兼容标准+URL-safe base64，自动补 padding
  - `pub fn decode_base64_url_string(s: &str) -> Result<String, ParseError>` — 解码成 UTF-8 字符串
  - `pub fn percent_decode(s: &str) -> Result<String, ParseError>`
  - `pub fn split_authority(auth: &str) -> (&str, &str)` — 在最后一个 `@` 处拆分 userinfo 和 host:port
  - `pub fn parse_host_port(s: &str) -> Result<(String, u16), ParseError>`

- [ ] **Step 1: 写失败测试**

```rust
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
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p proxy-core uri::tests`
Expected: FAIL — `cannot find function decode_base64_url_string`

- [ ] **Step 3: 实现 uri.rs**

```rust
// crates/proxy-core/src/uri.rs
use crate::error::ParseError;

pub fn decode_base64_url(s: &str) -> Result<Vec<u8>, ParseError> {
    use base64::Engine;
    let mut t = s.trim().to_string();
    // 转成标准 alphabet 并补 padding
    t = t.replace('-', "+").replace('_', "/");
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
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p proxy-core uri::tests`
Expected: PASS（7 个测试）

- [ ] **Step 5: Commit**

```bash
git add crates/proxy-core/src/uri.rs
git commit -m "feat(proxy-core): generic URI utilities"
```

---

### Task 3: ss:// 协议解析与序列化

**Files:**
- Create: `crates/proxy-core/src/protocols/mod.rs`
- Create: `crates/proxy-core/src/protocols/ss.rs`
- Test: `crates/proxy-core/tests/protocol_ss.rs`

**Interfaces:**
- Consumes: `ProxyNode`, `Crypto`, `ParseError`, `SerializeError`, `uri::{decode_base64_url_string, percent_decode, split_authority, parse_host_port}`
- Produces:
  - `pub fn parse_ss(uri: &str) -> Result<ProxyNode, ParseError>`
  - `pub fn serialize_ss(node: &ProxyNode) -> Result<String, SerializeError>`
  - `pub fn is_ss(uri: &str) -> bool`

**SIP002 格式规格（本 Task 依据）：**
```
ss://BASE64URL(method:password)@host:port#tag        # legacy userinfo
ss://method:password@host:port/?plugin=...#tag      # SIP002 plaintext（AEAD-2022）
ss://BASE64URL(method:password@host:port)#tag        # 部分客户端（v2rayN）也接受
```

- [ ] **Step 1: 写失败测试 `tests/protocol_ss.rs`**

```rust
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
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p proxy-core --test protocol_ss`
Expected: FAIL — `cannot find module protocols::ss` / `cannot find function parse_ss`

- [ ] **Step 3: 创建 protocols/mod.rs**

```rust
// crates/proxy-core/src/protocols/mod.rs
pub mod http;
pub mod hysteria;
pub mod hysteria2;
pub mod ss;
pub mod ssr;
pub mod socks5;
pub mod trojan;
pub mod tuic;
pub mod vless;
pub mod vmess;
pub mod wireguard;
```

- [ ] **Step 4: 实现 ss.rs**

```rust
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
    let (server, port) = parse_host_port(hostport)?;

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
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test -p proxy-core --test protocol_ss`
Expected: PASS（6 个测试）

- [ ] **Step 6: Commit**

```bash
git add crates/proxy-core/src/protocols/
git commit -m "feat(proxy-core): ss protocol parse and serialize (SIP002)"
```

---

### Task 4: ssr:// 协议解析与序列化

**Files:**
- Create: `crates/proxy-core/src/protocols/ssr.rs`
- Test: `crates/proxy-core/tests/protocol_ssr.rs`

**Interfaces:**
- Consumes: `ProxyNode`, `Crypto`, `ParseError`, `SerializeError`, `uri::{decode_base64_url_string, percent_decode, parse_host_port, split_authority}`
- Produces:
  - `pub fn is_ssr(uri: &str) -> bool`
  - `pub fn parse_ssr(uri: &str) -> Result<ProxyNode, ParseError>`
  - `pub fn serialize_ssr(node: &ProxyNode) -> Result<String, SerializeError>`

**SSR 格式规格：**
```
ssr://base64url(server:port:protocol:method:obfs:base64url(password)/?obfsparam=base64url(...)&protoparam=base64url(...)&remarks=base64url(name)&group=...)
```
注意：password 是 base64，obfsparam/protoparam/remarks 都是各自 base64 编码，整体先 base64url 编码（不带 padding）。

- [ ] **Step 1: 写失败测试 `tests/protocol_ssr.rs`**

```rust
use proxy_core::model::{Crypto, Protocol};
use proxy_core::protocols::ssr::{parse_ssr, serialize_ssr};

// 对应明文: 1.2.3.4:8388:auth_aes128_md5:aes-256-cfb:plain:pass / remarks=US-01
const SSR: &str = "ssr://MS4yLjMuNDo4Mzg4OmF1dGhfYWVzMTI4X21kNTphZXMtMjU2LWNmYjpwbGFpbjpwYXNzLz9vYnZzcGFyYW09JmlkPTE&remarks=VVMtMDE";

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
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p proxy-core --test protocol_ssr`
Expected: FAIL — `cannot find function parse_ssr`

- [ ] **Step 3: 实现 ssr.rs**

```rust
// crates/proxy-core/src/protocols/ssr.rs
use crate::error::{ParseError, SerializeError};
use crate::model::{Crypto, ProxyNode};
use crate::uri::{decode_base64_url, decode_base64_url_string, parse_host_port};

pub fn is_ssr(uri: &str) -> bool {
    uri.starts_with("ssr://")
}

pub fn parse_ssr(uri: &str) -> Result<ProxyNode, ParseError> {
    let payload = uri
        .strip_prefix("ssr://")
        .ok_or(ParseError::UnsupportedProtocol)?;
    // payload 整体是 base64url。可能带 query 内嵌，但都在 base64 之内。
    let decoded = decode_base64_url_string(payload)?;
    parse_ssr_decoded(&decoded)
}

fn parse_ssr_decoded(decoded: &str) -> Result<ProxyNode, ParseError> {
    let (main, query) = match decoded.find('/') {
        Some(i) => (&decoded[..i], &decoded[i..]),
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

    // 解析 query 中的 remarks/group 等
    let mut name = String::new();
    for kv in query.split('&') {
        if let Some((k, v)) = kv.split_once('=') {
            let v = decode_base64_url_string(v).unwrap_or_default();
            if k == "remarks" {
                name = v;
            }
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
    let crypto = node.crypto.as_ref().ok_or(SerializeError::MissingField("crypto"))?;
    let password = node.password.as_ref().ok_or(SerializeError::MissingField("password"))?;
    let pass_b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, password.as_bytes());
    let remarks_b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, node.name.as_bytes());
    let plain = format!(
        "{}:{}:{}:{}:{}:{}/?remarks={}",
        node.server, node.port, "auth_aes128_md5", crypto.as_str(), "plain", pass_b64, remarks_b64
    );
    // 整体 base64url（去 padding）
    let b64 = base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, plain.as_bytes());
    Ok(format!("ssr://{}", b64))
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p proxy-core --test protocol_ssr`
Expected: PASS（3 个测试）

- [ ] **Step 5: Commit**

```bash
git add crates/proxy-core/src/protocols/ssr.rs
git commit -m "feat(proxy-core): ssr protocol parse and serialize"
```

---

### Task 5: socks5:// 与 http:// 传统代理

**Files:**
- Create: `crates/proxy-core/src/protocols/socks5.rs`
- Create: `crates/proxy-core/src/protocols/http.rs`
- Test: `crates/proxy-core/tests/protocol_legacy.rs`

**Interfaces:**
- Consumes: `ProxyNode`, `ParseError`, `SerializeError`, `uri::{percent_decode, split_authority, parse_host_port}`
- Produces:
  - `pub fn is_socks5(uri: &str) -> bool`, `pub fn parse_socks5(uri: &str) -> Result<ProxyNode, ParseError>`, `pub fn serialize_socks5(node: &ProxyNode) -> Result<String, SerializeError>`
  - `pub fn is_http(uri: &str) -> bool`, `pub fn parse_http(uri: &str) -> Result<ProxyNode, ParseError>`, `pub fn serialize_http(node: &ProxyNode) -> Result<String, SerializeError>`

**格式规格：**
```
socks5://[user:pass@]host:port#tag
http://[user:pass@]host:port#tag
```

- [ ] **Step 1: 写失败测试 `tests/protocol_legacy.rs`**

```rust
use proxy_core::model::{Protocol, ProxyNode};
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
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p proxy-core --test protocol_legacy`
Expected: FAIL — `cannot find function parse_socks5`

- [ ] **Step 3: 实现 socks5.rs**

```rust
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
    let (user, password) = if userinfo.is_empty() {
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
```

- [ ] **Step 4: 实现 http.rs**（与 socks5.rs 同构，仅协议前缀为 `http://`、kind 为 `Protocol::Http`）

```rust
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
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test -p proxy-core --test protocol_legacy`
Expected: PASS（7 个测试）

- [ ] **Step 6: Commit**

```bash
git add crates/proxy-core/src/protocols/socks5.rs crates/proxy-core/src/protocols/http.rs
git commit -m "feat(proxy-core): socks5 and http legacy proxy protocols"
```

---

### Task 6: vmess:// 协议解析与序列化

**Files:**
- Create: `crates/proxy-core/src/protocols/vmess.rs`
- Test: `crates/proxy-core/tests/protocol_vmess.rs`

**Interfaces:**
- Consumes: `ProxyNode`, `TlsSettings`, `Transport`, `WebsocketConfig`, `GrpcConfig`, `HttpUpgradeConfig`, `ParseError`, `SerializeError`, `uri::{decode_base64_url_string, percent_decode}`
- Produces:
  - `pub fn is_vmess(uri: &str) -> bool`
  - `pub fn parse_vmess(uri: &str) -> Result<ProxyNode, ParseError>`
  - `pub fn serialize_vmess(node: &ProxyNode) -> Result<String, SerializeError>`

**v2rayN 新格式（本 Task 依据）：**
```
vmess://base64url({
  "v": "2", "ps": "name", "add": "server", "port": "443", "id": "uuid",
  "aid": "0", "net": "ws|grpc|tcp|http|h2", "type": "none|http",
  "host": "sni-or-host", "path": "/path", "tls": "tls|none",
  "sni": "...", "alpn": "...", "fp": "...", "security": "auto|tls",
  "allowInsecure": false
})
```
同时也兼容老格式：`vmess://` + 直接 base64 JSON，以及部分客户端用 `#tag` 形式。

- [ ] **Step 1: 写失败测试 `tests/protocol_vmess.rs`**

```rust
use proxy_core::model::Protocol;
use proxy_core::protocols::vmess::{parse_vmess, serialize_vmess};

// 对应 JSON: {"v":"2","ps":"SG-01","add":"1.2.3.4","port":"443","id":"uuid-1111-1111","aid":"0","net":"ws","type":"none","host":"cdn.example.com","path":"/ws","tls":"tls"}
const VMESS: &str = "vmess://eyJ2IjoiMiIsInBzIjoiU0ctMDEiLCJhZGQiOiIxLjIuMy40IiwicG9ydCI6IjQ0MyIsImlkIjoidXVpZC0xMTExLTExMTEiLCJhaWQiOiIwIiwibmV0Ijoid3MiLCJ0eXBlIjoibm9uZSIsImhvc3QiOiJjZG4uZXhhbXBsZS5jb20iLCJwYXRoIjoiL3dzIiwidGxzIjoidGxzIn0=";

#[test]
fn parse_vmess_ws_tls() {
    let n = parse_vmess(VMESS).unwrap();
    assert_eq!(n.kind, Protocol::Vmess);
    assert_eq!(n.name, "SG-01");
    assert_eq!(n.server, "1.2.3.4");
    assert_eq!(n.port, 443);
    assert_eq!(n.uuid.as_deref(), Some("uuid-1111-1111"));
    assert_eq!(n.alter_id, Some(0));
    let tls = n.tls.as_ref().unwrap();
    assert!(tls.enabled);
    assert_eq!(tls.sni.as_deref(), Some("cdn.example.com"));
    let ws = n.transport.as_ref().and_then(|t| t.websocket.as_ref()).unwrap();
    assert_eq!(ws.path, "/ws");
}

#[test]
fn parse_vmess_tcp_none() {
    // {"v":"2","ps":"T","add":"1.2.3.4","port":"443","id":"u","aid":"0","net":"tcp","tls":"none"}
    let uri = "vmess://eyJ2IjoiMiIsInBzIjoiVCIsImFkZCI6IjEuMi4zLjQiLCJwb3J0IjoiNDQzIiwiaWQiOiJ1IiwiYWlkIjoiMCIsIm5ldCI6InRjcCIsInRscyI6Im5vbmUifQ==";
    let n = parse_vmess(uri).unwrap();
    assert!(n.tls.is_none());
    assert!(n.transport.is_none());
}

#[test]
fn vmess_roundtrip() {
    let n = parse_vmess(VMESS).unwrap();
    let out = serialize_vmess(&n).unwrap();
    assert!(out.starts_with("vmess://"));
    let n2 = parse_vmess(&out).unwrap();
    assert_eq!(n2.server, n.server);
    assert_eq!(n2.port, n.port);
    assert_eq!(n2.uuid, n.uuid);
    assert_eq!(n2.transport, n.transport);
    assert_eq!(n2.tls, n.tls);
}

#[test]
fn vmess_invalid() {
    assert!(parse_vmess("vmess://").is_err());
    assert!(parse_vmess("vmess://bm90LXN0YW5kYXJkLWpzb24=").is_err()); // "not-standard-json"
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p proxy-core --test protocol_vmess`
Expected: FAIL — `cannot find function parse_vmess`

- [ ] **Step 3: 实现 vmess.rs**

```rust
// crates/proxy-core/src/protocols/vmess.rs
use crate::error::{ParseError, SerializeError};
use crate::model::{
    GrpcConfig, HttpUpgradeConfig, Protocol, ProxyNode, TlsSettings, Transport, WebsocketConfig,
};
use crate::uri::decode_base64_url_string;
use serde_json::json;

pub fn is_vmess(uri: &str) -> bool {
    uri.starts_with("vmess://")
}

pub fn parse_vmess(uri: &str) -> Result<ProxyNode, ParseError> {
    let payload = uri.strip_prefix("vmess://").ok_or(ParseError::UnsupportedProtocol)?;
    let json_str = decode_base64_url_string(payload)?;
    let v: serde_json::Value = serde_json::from_str(&json_str)
        .map_err(|_| ParseError::InvalidUri(uri.to_string()))?;

    let get = |k: &str| -> Option<String> {
        v.get(k).and_then(|x| match x {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
    };

    let name = get("ps").unwrap_or_default();
    let server = get("add").ok_or(ParseError::MissingField("add"))?;
    let port: u16 = get("port")
        .and_then(|p| p.parse().ok())
        .ok_or(ParseError::InvalidPort)?;
    let uuid = get("id");
    let alter_id: Option<u16> = get("aid").and_then(|a| a.parse().ok());
    let net = get("net").unwrap_or_else(|| "tcp".into());
    let tls_enabled = matches!(get("tls").as_deref(), Some("tls") | Some("xtls") | Some("reality")) || get("security").as_deref() == Some("tls");
    let host = get("host");
    let path = get("path").unwrap_or_default();
    let sni = get("sni").or(host.clone());
    let alpn = get("alpn").map(|a| a.split(',').map(|s| s.to_string()).collect()).unwrap_or_default();
    let insecure = get("allowInsecure").map(|s| s == "1" || s == "true").unwrap_or(false);
    let fp = get("fp");

    let tls = if tls_enabled {
        Some(TlsSettings {
            enabled: true,
            sni,
            alpn,
            insecure,
            fingerprint: fp,
        })
    } else {
        None
    };

    let transport = match net.as_str() {
        "ws" | "websocket" => Some(Transport {
            websocket: Some(WebsocketConfig {
                path,
                host: host,
                headers: Default::default(),
            }),
            ..Default::default()
        }),
        "grpc" => Some(Transport {
            grpc: Some(GrpcConfig { service_name: path.trim_start_matches('/').to_string() }),
            ..Default::default()
        }),
        "httpupgrade" | "http" => Some(Transport {
            http_upgrade: Some(HttpUpgradeConfig {
                path,
                host,
            }),
            ..Default::default()
        }),
        _ => None,
    };

    Ok(ProxyNode {
        name,
        kind: Protocol::Vmess,
        server,
        port,
        uuid,
        alter_id,
        tls,
        transport,
        ..Default::default()
    })
}

pub fn serialize_vmess(node: &ProxyNode) -> Result<String, SerializeError> {
    if node.kind != Protocol::Vmess {
        return Err(SerializeError::UnsupportedProtocol(node.kind.as_str()));
    }
    let uuid = node.uuid.clone().unwrap_or_default();
    let (net, host, path, type_): (&str, String, String, &str) =
        match &node.transport {
            Some(t) if t.websocket.is_some() => {
                let ws = t.websocket.as_ref().unwrap();
                ("ws", ws.host.clone().unwrap_or_default(), ws.path.clone(), "none")
            }
            Some(t) if t.grpc.is_some() => {
                let g = t.grpc.as_ref().unwrap();
                ("grpc", String::new(), format!("/{}", g.service_name), "none")
            }
            Some(t) if t.http_upgrade.is_some() => {
                let h = t.http_upgrade.as_ref().unwrap();
                ("http", h.host.clone().unwrap_or_default(), h.path.clone(), "none")
            }
            _ => ("tcp", String::new(), String::new(), "none"),
        };

    let tls_str = if node.tls.as_ref().map(|t| t.enabled).unwrap_or(false) { "tls" } else { "none" };
    let sni = node.tls.as_ref().and_then(|t| t.sni.clone()).unwrap_or_default();
    let alpn = node.tls.as_ref().map(|t| t.alpn.join(",")).unwrap_or_default();
    let fp = node.tls.as_ref().and_then(|t| t.fingerprint.clone()).unwrap_or_default();
    let insecure = node.tls.as_ref().map(|t| t.insecure).unwrap_or(false);

    let obj = json!({
        "v": "2", "ps": node.name, "add": node.server, "port": node.port.to_string(),
        "id": uuid, "aid": node.alter_id.unwrap_or(0).to_string(),
        "net": net, "type": type_, "host": host, "path": path,
        "tls": tls_str, "sni": sni, "alpn": alpn, "fp": fp,
        "allowInsecure": if insecure { "1" } else { "0" },
    });
    let s = serde_json::to_string(&obj).map_err(|_| SerializeError::MissingField("json"))?;
    let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, s.as_bytes());
    Ok(format!("vmess://{}", b64))
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p proxy-core --test protocol_vmess`
Expected: PASS（4 个测试）

> **注意**：vmess 需要 `serde_json` 依赖。加入 `crates/proxy-core/Cargo.toml`：
> ```toml
> serde_json = "1"
> ```

- [ ] **Step 5: Commit**

```bash
git add crates/proxy-core/src/protocols/vmess.rs crates/proxy-core/Cargo.toml
git commit -m "feat(proxy-core): vmess protocol parse and serialize (v2rayN JSON)"
```

---

### Task 7: vless:// 协议解析与序列化

**Files:**
- Create: `crates/proxy-core/src/protocols/vless.rs`
- Test: `crates/proxy-core/tests/protocol_vless.rs`

**Interfaces:**
- Consumes: `ProxyNode`, `TlsSettings`, `Transport`, `ParseError`, `SerializeError`, `uri::{percent_decode, split_authority, parse_host_port}`
- Produces:
  - `pub fn is_vless(uri: &str) -> bool`
  - `pub fn parse_vless(uri: &str) -> Result<ProxyNode, ParseError>`
  - `pub fn serialize_vless(node: &ProxyNode) -> Result<String, SerializeError>`

**vless 格式规格：**
```
vless://UUID@host:port?encryption=none&type=ws|tcp|grpc&security=tls|reality|none&sni=...&path=...&fp=...#tag
```

- [ ] **Step 1: 写失败测试 `tests/protocol_vless.rs`**

```rust
use proxy_core::model::Protocol;
use proxy_core::protocols::vless::{parse_vless, serialize_vless};

const VLESS_WS: &str = "vless://11111111-2222-3333-4444-555555555555@1.2.3.4:443?encryption=none&security=tls&sni=cdn.example.com&type=ws&path=%2Fws&fp=chrome#JP-01";
const VLESS_TCP: &str = "vless://11111111-2222-3333-4444-555555555555@1.2.3.4:443?encryption=none&security=none&type=tcp#JP-01";

#[test]
fn parse_vless_ws_tls() {
    let n = parse_vless(VLESS_WS).unwrap();
    assert_eq!(n.kind, Protocol::Vless);
    assert_eq!(n.uuid.as_deref(), Some("11111111-2222-3333-4444-555555555555"));
    assert_eq!(n.server, "1.2.3.4");
    assert_eq!(n.port, 443);
    assert_eq!(n.name, "JP-01");
    let tls = n.tls.as_ref().unwrap();
    assert!(tls.enabled);
    assert_eq!(tls.sni.as_deref(), Some("cdn.example.com"));
    assert_eq!(tls.fingerprint.as_deref(), Some("chrome"));
    let ws = n.transport.as_ref().and_then(|t| t.websocket.as_ref()).unwrap();
    assert_eq!(ws.path, "/ws");
}

#[test]
fn parse_vless_tcp_none() {
    let n = parse_vless(VLESS_TCP).unwrap();
    assert!(n.tls.is_none());
    assert!(n.transport.is_none());
}

#[test]
fn vless_roundtrip() {
    let n = parse_vless(VLESS_WS).unwrap();
    let out = serialize_vless(&n).unwrap();
    assert!(out.starts_with("vless://"));
    let n2 = parse_vless(&out).unwrap();
    assert_eq!(n2.uuid, n.uuid);
    assert_eq!(n2.server, n.server);
    assert_eq!(n2.port, n.port);
    assert_eq!(n2.transport, n.transport);
    assert_eq!(n2.tls, n.tls);
}

#[test]
fn vless_invalid() {
    assert!(parse_vless("vless://").is_err());
    assert!(parse_vless("vless://bad@host").is_err());
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p proxy-core --test protocol_vless`
Expected: FAIL — `cannot find function parse_vless`

- [ ] **Step 3: 实现 vless.rs**

```rust
// crates/proxy-core/src/protocols/vless.rs
use crate::error::{ParseError, SerializeError};
use crate::model::{
    GrpcConfig, HttpUpgradeConfig, Protocol, ProxyNode, TlsSettings, Transport, WebsocketConfig,
};
use crate::uri::{parse_host_port, percent_decode, split_authority};
use urlencoding::encode;

pub fn is_vless(uri: &str) -> bool {
    uri.starts_with("vless://")
}

pub fn parse_vless(uri: &str) -> Result<ProxyNode, ParseError> {
    let rest = uri.strip_prefix("vless://").ok_or(ParseError::UnsupportedProtocol)?;
    let (auth_and_query, fragment) = match rest.find('#') {
        Some(i) => (&rest[..i], Some(&rest[i + 1..])),
        None => (rest, None),
    };
    let (userinfo, host_query) = split_authority(auth_and_query);
    let (hostpart, query) = match host_query.find('?') {
        Some(i) => (&host_query[..i], Some(&host_query[i + 1..])),
        None => (host_query, None),
    };
    let (server, port) = parse_host_port(hostpart)?;
    let uuid = userinfo.to_string();
    if uuid.is_empty() {
        return Err(ParseError::MissingField("uuid"));
    }

    let mut security = String::new();
    let mut sni = None;
    let mut fp = None;
    let mut net = String::from("tcp");
    let mut path = String::new();
    let mut host = None;
    let mut alpn = Vec::new();
    let mut insecure = false;

    if let Some(q) = query {
        for kv in q.split('&') {
            let Some((k, v)) = kv.split_once('=') else { continue };
            let v = percent_decode(v).unwrap_or_default();
            match k {
                "security" => security = v,
                "sni" => sni = Some(v),
                "fp" => fp = Some(v),
                "type" => net = v,
                "path" => path = v,
                "host" => host = Some(v),
                "alpn" => alpn = v.split(',').map(|s| s.to_string()).collect(),
                "allowInsecure" => insecure = v == "1" || v == "true",
                _ => {}
            }
        }
    }

    let tls = if matches!(security.as_str(), "tls" | "reality" | "xtls") {
        Some(TlsSettings {
            enabled: true,
            sni: sni.or(host.clone()),
            alpn,
            insecure,
            fingerprint: fp,
        })
    } else {
        None
    };

    let transport = match net.as_str() {
        "ws" | "websocket" => Some(Transport {
            websocket: Some(WebsocketConfig { path, host, headers: Default::default() }),
            ..Default::default()
        }),
        "grpc" => Some(Transport {
            grpc: Some(GrpcConfig { service_name: path.trim_start_matches('/').to_string() }),
            ..Default::default()
        }),
        "httpupgrade" => Some(Transport {
            http_upgrade: Some(HttpUpgradeConfig { path, host }),
            ..Default::default()
        }),
        _ => None,
    };

    Ok(ProxyNode {
        name: fragment.map(|f| f.to_string()).unwrap_or_default(),
        kind: Protocol::Vless,
        server,
        port,
        uuid: Some(uuid),
        tls,
        transport,
        ..Default::default()
    })
}

pub fn serialize_vless(node: &ProxyNode) -> Result<String, SerializeError> {
    if node.kind != Protocol::Vless {
        return Err(SerializeError::UnsupportedProtocol(node.kind.as_str()));
    }
    let uuid = node.uuid.as_ref().ok_or(SerializeError::MissingField("uuid"))?;
    let mut out = format!("vless://{}@{}:{}?encryption=none", uuid, node.server, node.port);

    let (net, host, path) = match &node.transport {
        Some(t) if t.websocket.is_some() => {
            let ws = t.websocket.as_ref().unwrap();
            ("ws", ws.host.clone().unwrap_or_default(), ws.path.clone())
        }
        Some(t) if t.grpc.is_some() => {
            let g = t.grpc.as_ref().unwrap();
            ("grpc", String::new(), format!("/{}", g.service_name))
        }
        Some(t) if t.http_upgrade.is_some() => {
            let h = t.http_upgrade.as_ref().unwrap();
            ("http", h.host.clone().unwrap_or_default(), h.path.clone())
        }
        _ => ("tcp", String::new(), String::new()),
    };
    if net != "tcp" {
        out.push_str(&format!("&type={}", net));
    }
    if let Some(t) = &node.tls {
        if t.enabled {
            out.push_str("&security=tls");
        }
        if let Some(s) = &t.sni {
            out.push_str(&format!("&sni={}", encode(s)));
        }
        if let Some(fp) = &t.fingerprint {
            out.push_str(&format!("&fp={}", encode(fp)));
        }
        if !t.alpn.is_empty() {
            out.push_str(&format!("&alpn={}", encode(&t.alpn.join(","))));
        }
    }
    if !host.is_empty() {
        out.push_str(&format!("&host={}", encode(&host)));
    }
    if !path.is_empty() {
        out.push_str(&format!("&path={}", encode(&path)));
    }
    if !node.name.is_empty() {
        out.push('#');
        out.push_str(&node.name);
    }
    Ok(out)
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p proxy-core --test protocol_vless`
Expected: PASS（4 个测试）

- [ ] **Step 5: Commit**

```bash
git add crates/proxy-core/src/protocols/vless.rs
git commit -m "feat(proxy-core): vless protocol parse and serialize"
```

---

### Task 8: trojan:// 协议解析与序列化

**Files:**
- Create: `crates/proxy-core/src/protocols/trojan.rs`
- Test: `crates/proxy-core/tests/protocol_trojan.rs`

**Interfaces:**
- Consumes: `ProxyNode`, `TlsSettings`, `Transport`, `ParseError`, `SerializeError`, `uri::{percent_decode, split_authority, parse_host_port}`
- Produces:
  - `pub fn is_trojan(uri: &str) -> bool`
  - `pub fn parse_trojan(uri: &str) -> Result<ProxyNode, ParseError>`
  - `pub fn serialize_trojan(node: &ProxyNode) -> Result<String, SerializeError>`

**trojan 格式规格：**
```
trojan://password@host:port?security=tls|reality&sni=...&type=ws|tcp&path=...&fp=...#tag
```
password 可能含特殊字符需 percent-encode。

- [ ] **Step 1: 写失败测试 `tests/protocol_trojan.rs`**

```rust
use proxy_core::model::Protocol;
use proxy_core::protocols::trojan::{parse_trojan, serialize_trojan};

const TROJAN_TCP: &str = "trojan://pass%40word@1.2.3.4:443?security=tls&sni=example.com#KR-01";
const TROJAN_WS: &str = "trojan://abc123@1.2.3.4:443?security=tls&sni=example.com&type=ws&path=%2Ftr&host=example.com#KR-02";

#[test]
fn parse_trojan_tcp() {
    let n = parse_trojan(TROJAN_TCP).unwrap();
    assert_eq!(n.kind, Protocol::Trojan);
    assert_eq!(n.server, "1.2.3.4");
    assert_eq!(n.port, 443);
    assert_eq!(n.password.as_deref(), Some("pass@word"));
    assert_eq!(n.name, "KR-01");
    let tls = n.tls.as_ref().unwrap();
    assert!(tls.enabled);
    assert_eq!(tls.sni.as_deref(), Some("example.com"));
    assert!(n.transport.is_none());
}

#[test]
fn parse_trojan_ws() {
    let n = parse_trojan(TROJAN_WS).unwrap();
    let ws = n.transport.as_ref().and_then(|t| t.websocket.as_ref()).unwrap();
    assert_eq!(ws.path, "/tr");
    assert_eq!(ws.host.as_deref(), Some("example.com"));
}

#[test]
fn trojan_roundtrip() {
    let n = parse_trojan(TROJAN_TCP).unwrap();
    let out = serialize_trojan(&n).unwrap();
    assert!(out.starts_with("trojan://"));
    let n2 = parse_trojan(&out).unwrap();
    assert_eq!(n2.password, n.password);
    assert_eq!(n2.server, n.server);
    assert_eq!(n2.tls, n.tls);
}

#[test]
fn trojan_invalid() {
    assert!(parse_trojan("trojan://").is_err());
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p proxy-core --test protocol_trojan`
Expected: FAIL — `cannot find function parse_trojan`

- [ ] **Step 3: 实现 trojan.rs**（结构与 vless 类似，password 在 userinfo、无 uuid）

```rust
// crates/proxy-core/src/protocols/trojan.rs
use crate::error::{ParseError, SerializeError};
use crate::model::{
    GrpcConfig, HttpUpgradeConfig, Protocol, ProxyNode, TlsSettings, Transport, WebsocketConfig,
};
use crate::uri::{parse_host_port, percent_decode, split_authority};
use urlencoding::encode;

pub fn is_trojan(uri: &str) -> bool {
    uri.starts_with("trojan://")
}

pub fn parse_trojan(uri: &str) -> Result<ProxyNode, ParseError> {
    let rest = uri.strip_prefix("trojan://").ok_or(ParseError::UnsupportedProtocol)?;
    let (auth_and_query, fragment) = match rest.find('#') {
        Some(i) => (&rest[..i], Some(&rest[i + 1..])),
        None => (rest, None),
    };
    let (userinfo, host_query) = split_authority(auth_and_query);
    let (hostpart, query) = match host_query.find('?') {
        Some(i) => (&host_query[..i], Some(&host_query[i + 1..])),
        None => (host_query, None),
    };
    let (server, port) = parse_host_port(hostpart)?;
    let password = percent_decode(userinfo)?;
    if password.is_empty() {
        return Err(ParseError::MissingField("password"));
    }

    let mut security = String::new();
    let mut sni = None;
    let mut fp = None;
    let mut net = String::from("tcp");
    let mut path = String::new();
    let mut host = None;
    let mut alpn = Vec::new();
    let mut insecure = false;

    if let Some(q) = query {
        for kv in q.split('&') {
            let Some((k, v)) = kv.split_once('=') else { continue };
            let v = percent_decode(v).unwrap_or_default();
            match k {
                "security" => security = v,
                "sni" => sni = Some(v),
                "fp" => fp = Some(v),
                "type" => net = v,
                "path" => path = v,
                "host" => host = Some(v),
                "alpn" => alpn = v.split(',').map(|s| s.to_string()).collect(),
                "allowInsecure" => insecure = v == "1" || v == "true",
                _ => {}
            }
        }
    }

    let tls = if matches!(security.as_str(), "tls" | "reality" | "xtls") {
        Some(TlsSettings {
            enabled: true,
            sni: sni.or(host.clone()),
            alpn,
            insecure,
            fingerprint: fp,
        })
    } else {
        None
    };

    let transport = match net.as_str() {
        "ws" | "websocket" => Some(Transport {
            websocket: Some(WebsocketConfig { path, host, headers: Default::default() }),
            ..Default::default()
        }),
        "grpc" => Some(Transport {
            grpc: Some(GrpcConfig { service_name: path.trim_start_matches('/').to_string() }),
            ..Default::default()
        }),
        "httpupgrade" => Some(Transport {
            http_upgrade: Some(HttpUpgradeConfig { path, host }),
            ..Default::default()
        }),
        _ => None,
    };

    Ok(ProxyNode {
        name: fragment.map(|f| f.to_string()).unwrap_or_default(),
        kind: Protocol::Trojan,
        server,
        port,
        password: Some(password),
        tls,
        transport,
        ..Default::default()
    })
}

pub fn serialize_trojan(node: &ProxyNode) -> Result<String, SerializeError> {
    if node.kind != Protocol::Trojan {
        return Err(SerializeError::UnsupportedProtocol(node.kind.as_str()));
    }
    let password = node.password.as_ref().ok_or(SerializeError::MissingField("password"))?;
    let mut out = format!("trojan://{}@{}:{}", encode(password), node.server, node.port);

    let (net, host, path) = match &node.transport {
        Some(t) if t.websocket.is_some() => {
            let ws = t.websocket.as_ref().unwrap();
            ("ws", ws.host.clone().unwrap_or_default(), ws.path.clone())
        }
        Some(t) if t.grpc.is_some() => {
            let g = t.grpc.as_ref().unwrap();
            ("grpc", String::new(), format!("/{}", g.service_name))
        }
        Some(t) if t.http_upgrade.is_some() => {
            let h = t.http_upgrade.as_ref().unwrap();
            ("http", h.host.clone().unwrap_or_default(), h.path.clone())
        }
        _ => ("tcp", String::new(), String::new()),
    };
    if let Some(t) = &node.tls {
        if t.enabled {
            out.push_str("?security=tls");
        }
        if let Some(s) = &t.sni {
            out.push_str(&format!("&sni={}", encode(s)));
        }
        if let Some(fp) = &t.fingerprint {
            out.push_str(&format!("&fp={}", encode(fp)));
        }
        if !t.alpn.is_empty() {
            out.push_str(&format!("&alpn={}", encode(&t.alpn.join(","))));
        }
    }
    if net != "tcp" {
        out.push_str(&format!("&type={}", net));
    }
    if !host.is_empty() {
        out.push_str(&format!("&host={}", encode(&host)));
    }
    if !path.is_empty() {
        out.push_str(&format!("&path={}", encode(&path)));
    }
    if !node.name.is_empty() {
        out.push('#');
        out.push_str(&node.name);
    }
    Ok(out)
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p proxy-core --test protocol_trojan`
Expected: PASS（4 个测试）

- [ ] **Step 5: Commit**

```bash
git add crates/proxy-core/src/protocols/trojan.rs
git commit -m "feat(proxy-core): trojan protocol parse and serialize"
```

---

### Task 9: hysteria://（hysteria1）协议解析与序列化

**Files:**
- Create: `crates/proxy-core/src/protocols/hysteria.rs`
- Test: `crates/proxy-core/tests/protocol_hysteria.rs`

**Interfaces:**
- Consumes: `ProxyNode`, `Crypto`, `TlsSettings`, `ParseError`, `SerializeError`, `uri::{percent_decode, split_authority, parse_host_port}`
- Produces:
  - `pub fn is_hysteria(uri: &str) -> bool`
  - `pub fn parse_hysteria(uri: &str) -> Result<ProxyNode, ParseError>`
  - `pub fn serialize_hysteria(node: &ProxyNode) -> Result<String, SerializeError>`

**hysteria1 格式规格：**
```
hysteria://host:port?protocol=udp&auth=password&upmbps=100&downmbps=100&obfs=xorsalsa20&sni=...&insecure=1&alpn=hysteria#name
```
`auth` 是共享密钥；hysteria1 需要 `upmbps`/`downmbps` 参数（v1 特有）。

- [ ] **Step 1: 写失败测试 `tests/protocol_hysteria.rs`**

```rust
use proxy_core::model::Protocol;
use proxy_core::protocols::hysteria::{parse_hysteria, serialize_hysteria};

const HY1: &str = "hysteria://1.2.3.4:36712?protocol=udp&auth=secret123&upmbps=100&downmbps=100&obfs=xorsalsa20&sni=example.com&insecure=1&alpn=hysteria#JP-01";

#[test]
fn parse_hysteria_basic() {
    let n = parse_hysteria(HY1).unwrap();
    assert_eq!(n.kind, Protocol::Hysteria);
    assert_eq!(n.server, "1.2.3.4");
    assert_eq!(n.port, 36712);
    assert_eq!(n.password.as_deref(), Some("secret123"));
    assert_eq!(n.name, "JP-01");
    let tls = n.tls.as_ref().unwrap();
    assert!(tls.enabled);
    assert!(tls.insecure);
    assert_eq!(tls.sni.as_deref(), Some("example.com"));
    assert_eq!(tls.alpn, vec!["hysteria".to_string()]);
}

#[test]
fn hysteria_roundtrip() {
    let n = parse_hysteria(HY1).unwrap();
    let out = serialize_hysteria(&n).unwrap();
    assert!(out.starts_with("hysteria://"));
    let n2 = parse_hysteria(&out).unwrap();
    assert_eq!(n2.password, n.password);
    assert_eq!(n2.server, n.server);
    assert_eq!(n2.tls, n.tls);
}

#[test]
fn hysteria_invalid() {
    assert!(parse_hysteria("hysteria://").is_err());
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p proxy-core --test protocol_hysteria`
Expected: FAIL — `cannot find function parse_hysteria`

- [ ] **Step 3: 实现 hysteria.rs**

```rust
// crates/proxy-core/src/protocols/hysteria.rs
use crate::error::{ParseError, SerializeError};
use crate::model::{Protocol, ProxyNode, TlsSettings};
use crate::uri::{parse_host_port, percent_decode};
use urlencoding::encode;

pub fn is_hysteria(uri: &str) -> bool {
    uri.starts_with("hysteria://")
}

pub fn parse_hysteria(uri: &str) -> Result<ProxyNode, ParseError> {
    let rest = uri.strip_prefix("hysteria://").ok_or(ParseError::UnsupportedProtocol)?;
    let (host_query, fragment) = match rest.find('#') {
        Some(i) => (&rest[..i], Some(&rest[i + 1..])),
        None => (rest, None),
    };
    let (hostpart, query) = match host_query.find('?') {
        Some(i) => (&host_query[..i], Some(&host_query[i + 1..])),
        None => (host_query, None),
    };
    let (server, port) = parse_host_port(hostpart)?;

    let mut auth = None;
    let mut sni = None;
    let mut alpn = Vec::new();
    let mut insecure = false;

    if let Some(q) = query {
        for kv in q.split('&') {
            let Some((k, v)) = kv.split_once('=') else { continue };
            let v = percent_decode(v).unwrap_or_default();
            match k {
                "auth" => auth = Some(v),
                "sni" => sni = Some(v),
                "alpn" => alpn = vec![v],
                "insecure" => insecure = v == "1" || v == "true",
                _ => {}
            }
        }
    }

    Ok(ProxyNode {
        name: fragment.map(|f| f.to_string()).unwrap_or_default(),
        kind: Protocol::Hysteria,
        server,
        port,
        password: auth,
        tls: Some(TlsSettings {
            enabled: true,
            sni,
            alpn,
            insecure,
            fingerprint: None,
        }),
        ..Default::default()
    })
}

pub fn serialize_hysteria(node: &ProxyNode) -> Result<String, SerializeError> {
    if node.kind != Protocol::Hysteria {
        return Err(SerializeError::UnsupportedProtocol(node.kind.as_str()));
    }
    let auth = node.password.as_deref().unwrap_or_default();
    let mut out = format!("hysteria://{}:{}?protocol=udp&auth={}", node.server, node.port, encode(auth));
    if let Some(t) = &node.tls {
        if let Some(s) = &t.sni {
            out.push_str(&format!("&sni={}", encode(s)));
        }
        if let Some(a) = t.alpn.first() {
            out.push_str(&format!("&alpn={}", encode(a)));
        }
        if t.insecure {
            out.push_str("&insecure=1");
        }
    }
    out.push_str("&upmbps=100&downmbps=100&obfs=xorsalsa20");
    if !node.name.is_empty() {
        out.push('#');
        out.push_str(&node.name);
    }
    Ok(out)
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p proxy-core --test protocol_hysteria`
Expected: PASS（3 个测试）

- [ ] **Step 5: Commit**

```bash
git add crates/proxy-core/src/protocols/hysteria.rs
git commit -m "feat(proxy-core): hysteria1 protocol parse and serialize"
```

---

### Task 10: hysteria2:// 协议解析与序列化

**Files:**
- Create: `crates/proxy-core/src/protocols/hysteria2.rs`
- Test: `crates/proxy-core/tests/protocol_hysteria2.rs`

**Interfaces:**
- Consumes: `ProxyNode`, `Crypto`, `TlsSettings`, `ParseError`, `SerializeError`, `uri::{percent_decode, split_authority, parse_host_port}`
- Produces:
  - `pub fn is_hysteria2(uri: &str) -> bool`
  - `pub fn parse_hysteria2(uri: &str) -> Result<ProxyNode, ParseError>`
  - `pub fn serialize_hysteria2(node: &ProxyNode) -> Result<String, SerializeError>`

**hysteria2 格式规格：**
```
hysteria2://password@host:port?sni=...&insecure=1&obfs=salamander&obfs-password=xxx&pinSHA256=...#name
```
hy2 的 `password` 位于 userinfo（percent-encoded），query 里是 `sni`、`insecure`、`obfs`、`obfs-password` 等。

- [ ] **Step 1: 写失败测试 `tests/protocol_hysteria2.rs`**

```rust
use proxy_core::model::Protocol;
use proxy_core::protocols::hysteria2::{parse_hysteria2, serialize_hysteria2};

const HY2: &str = "hysteria2://pass%40word@1.2.3.4:8443?sni=example.com&insecure=0&obfs=salamander&obfs-password=obs#US-02";

#[test]
fn parse_hysteria2_basic() {
    let n = parse_hysteria2(HY2).unwrap();
    assert_eq!(n.kind, Protocol::Hysteria2);
    assert_eq!(n.server, "1.2.3.4");
    assert_eq!(n.port, 8443);
    assert_eq!(n.password.as_deref(), Some("pass@word"));
    assert_eq!(n.name, "US-02");
    let tls = n.tls.as_ref().unwrap();
    assert!(tls.enabled);
    assert_eq!(tls.sni.as_deref(), Some("example.com"));
    assert!(!tls.insecure);
}

#[test]
fn hysteria2_insecure_flag() {
    let n = parse_hysteria2("hysteria2://p@1.2.3.4:8443?insecure=1#T").unwrap();
    assert!(n.tls.as_ref().unwrap().insecure);
}

#[test]
fn hysteria2_roundtrip() {
    let n = parse_hysteria2(HY2).unwrap();
    let out = serialize_hysteria2(&n).unwrap();
    assert!(out.starts_with("hysteria2://"));
    let n2 = parse_hysteria2(&out).unwrap();
    assert_eq!(n2.password, n.password);
    assert_eq!(n2.server, n.server);
    assert_eq!(n2.tls, n.tls);
}

#[test]
fn hysteria2_invalid() {
    assert!(parse_hysteria2("hysteria2://").is_err());
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p proxy-core --test protocol_hysteria2`
Expected: FAIL — `cannot find function parse_hysteria2`

- [ ] **Step 3: 实现 hysteria2.rs**

```rust
// crates/proxy-core/src/protocols/hysteria2.rs
use crate::error::{ParseError, SerializeError};
use crate::model::{Protocol, ProxyNode, TlsSettings};
use crate::uri::{parse_host_port, percent_decode, split_authority};
use urlencoding::encode;

pub fn is_hysteria2(uri: &str) -> bool {
    uri.starts_with("hysteria2://") || uri.starts_with("hy2://")
}

pub fn parse_hysteria2(uri: &str) -> Result<ProxyNode, ParseError> {
    let rest = uri
        .strip_prefix("hysteria2://")
        .or_else(|| uri.strip_prefix("hy2://"))
        .ok_or(ParseError::UnsupportedProtocol)?;
    let (auth_query, fragment) = match rest.find('#') {
        Some(i) => (&rest[..i], Some(&rest[i + 1..])),
        None => (rest, None),
    };
    let (userinfo, host_query) = split_authority(auth_query);
    let (hostpart, query) = match host_query.find('?') {
        Some(i) => (&host_query[..i], Some(&host_query[i + 1..])),
        None => (host_query, None),
    };
    let (server, port) = parse_host_port(hostpart)?;
    let password = if userinfo.is_empty() { None } else { Some(percent_decode(userinfo)?) };

    let mut sni = None;
    let mut alpn = Vec::new();
    let mut insecure = false;

    if let Some(q) = query {
        for kv in q.split('&') {
            let Some((k, v)) = kv.split_once('=') else { continue };
            let v = percent_decode(v).unwrap_or_default();
            match k {
                "sni" => sni = Some(v),
                "alpn" => alpn = vec![v],
                "insecure" => insecure = v == "1" || v == "true",
                _ => {}
            }
        }
    }

    Ok(ProxyNode {
        name: fragment.map(|f| f.to_string()).unwrap_or_default(),
        kind: Protocol::Hysteria2,
        server,
        port,
        password,
        tls: Some(TlsSettings {
            enabled: true,
            sni,
            alpn,
            insecure,
            fingerprint: None,
        }),
        ..Default::default()
    })
}

pub fn serialize_hysteria2(node: &ProxyNode) -> Result<String, SerializeError> {
    if node.kind != Protocol::Hysteria2 {
        return Err(SerializeError::UnsupportedProtocol(node.kind.as_str()));
    }
    let mut out = format!(
        "hysteria2://{}@{}:{}",
        encode(node.password.as_deref().unwrap_or_default()),
        node.server,
        node.port
    );
    if let Some(t) = &node.tls {
        if let Some(s) = &t.sni {
            out.push_str(&format!("?sni={}", encode(s)));
        }
        if let Some(a) = t.alpn.first() {
            out.push_str(&format!("&alpn={}", encode(a)));
        }
        if t.insecure {
            out.push_str("&insecure=1");
        }
    }
    if !node.name.is_empty() {
        out.push('#');
        out.push_str(&node.name);
    }
    Ok(out)
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p proxy-core --test protocol_hysteria2`
Expected: PASS（4 个测试）

- [ ] **Step 5: Commit**

```bash
git add crates/proxy-core/src/protocols/hysteria2.rs
git commit -m "feat(proxy-core): hysteria2 protocol parse and serialize"
```

---

### Task 11: tuic:// 协议解析与序列化

**Files:**
- Create: `crates/proxy-core/src/protocols/tuic.rs`
- Test: `crates/proxy-core/tests/protocol_tuic.rs`

**Interfaces:**
- Consumes: `ProxyNode`, `TlsSettings`, `ParseError`, `SerializeError`, `uri::{percent_decode, split_authority, parse_host_port}`
- Produces:
  - `pub fn is_tuic(uri: &str) -> bool`
  - `pub fn parse_tuic(uri: &str) -> Result<ProxyNode, ParseError>`
  - `pub fn serialize_tuic(node: &ProxyNode) -> Result<String, SerializeError>`

**tuic 格式规格（tuic 官方 URI）：**
```
tuic://uuid:password@host:port?congestion_control=bbr&udp_relay_mode=native&alpn=h3&sni=...&allow_insecure=1#name
```
`uuid:password` 在 userinfo 中。

- [ ] **Step 1: 写失败测试 `tests/protocol_tuic.rs`**

```rust
use proxy_core::model::Protocol;
use proxy_core::protocols::tuic::{parse_tuic, serialize_tuic};

const TUIC: &str = "tuic://11111111-2222-3333-4444-555555555555:pass%40word@1.2.3.4:443?congestion_control=bbr&udp_relay_mode=native&alpn=h3&sni=example.com&allow_insecure=1#TW-01";

#[test]
fn parse_tuic_basic() {
    let n = parse_tuic(TUIC).unwrap();
    assert_eq!(n.kind, Protocol::Tuic);
    assert_eq!(n.uuid.as_deref(), Some("11111111-2222-3333-4444-555555555555"));
    assert_eq!(n.password.as_deref(), Some("pass@word"));
    assert_eq!(n.server, "1.2.3.4");
    assert_eq!(n.port, 443);
    assert_eq!(n.name, "TW-01");
    let tls = n.tls.as_ref().unwrap();
    assert!(tls.enabled);
    assert!(tls.insecure);
    assert_eq!(tls.alpn, vec!["h3".to_string()]);
}

#[test]
fn tuic_roundtrip() {
    let n = parse_tuic(TUIC).unwrap();
    let out = serialize_tuic(&n).unwrap();
    assert!(out.starts_with("tuic://"));
    let n2 = parse_tuic(&out).unwrap();
    assert_eq!(n2.uuid, n.uuid);
    assert_eq!(n2.password, n.password);
    assert_eq!(n2.tls, n.tls);
}

#[test]
fn tuic_invalid() {
    assert!(parse_tuic("tuic://").is_err());
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p proxy-core --test protocol_tuic`
Expected: FAIL — `cannot find function parse_tuic`

- [ ] **Step 3: 实现 tuic.rs**

```rust
// crates/proxy-core/src/protocols/tuic.rs
use crate::error::{ParseError, SerializeError};
use crate::model::{Protocol, ProxyNode, TlsSettings};
use crate::uri::{parse_host_port, percent_decode, split_authority};
use urlencoding::encode;

pub fn is_tuic(uri: &str) -> bool {
    uri.starts_with("tuic://")
}

pub fn parse_tuic(uri: &str) -> Result<ProxyNode, ParseError> {
    let rest = uri.strip_prefix("tuic://").ok_or(ParseError::UnsupportedProtocol)?;
    let (auth_query, fragment) = match rest.find('#') {
        Some(i) => (&rest[..i], Some(&rest[i + 1..])),
        None => (rest, None),
    };
    let (userinfo, host_query) = split_authority(auth_query);
    let (hostpart, query) = match host_query.find('?') {
        Some(i) => (&host_query[..i], Some(&host_query[i + 1..])),
        None => (host_query, None),
    };
    let (server, port) = parse_host_port(hostpart)?;

    // userinfo = uuid:password
    let (uuid, password) = userinfo.split_once(':').ok_or(ParseError::MissingField("uuid"))?;
    let password = percent_decode(password)?;

    let mut sni = None;
    let mut alpn = Vec::new();
    let mut insecure = false;

    if let Some(q) = query {
        for kv in q.split('&') {
            let Some((k, v)) = kv.split_once('=') else { continue };
            let v = percent_decode(v).unwrap_or_default();
            match k {
                "sni" => sni = Some(v),
                "alpn" => alpn = v.split(',').map(|s| s.to_string()).collect(),
                "allow_insecure" | "insecure" => insecure = v == "1" || v == "true",
                _ => {}
            }
        }
    }

    Ok(ProxyNode {
        name: fragment.map(|f| f.to_string()).unwrap_or_default(),
        kind: Protocol::Tuic,
        server,
        port,
        uuid: Some(uuid.to_string()),
        password: Some(password),
        tls: Some(TlsSettings {
            enabled: true,
            sni,
            alpn,
            insecure,
            fingerprint: None,
        }),
        ..Default::default()
    })
}

pub fn serialize_tuic(node: &ProxyNode) -> Result<String, SerializeError> {
    if node.kind != Protocol::Tuic {
        return Err(SerializeError::UnsupportedProtocol(node.kind.as_str()));
    }
    let uuid = node.uuid.as_deref().unwrap_or_default();
    let password = encode(node.password.as_deref().unwrap_or_default());
    let mut out = format!("tuic://{}:{}@{}:{}", uuid, password, node.server, node.port);
    out.push_str("?congestion_control=bbr&udp_relay_mode=native");
    if let Some(t) = &node.tls {
        if let Some(s) = &t.sni {
            out.push_str(&format!("&sni={}", encode(s)));
        }
        if !t.alpn.is_empty() {
            out.push_str(&format!("&alpn={}", encode(&t.alpn.join(","))));
        }
        if t.insecure {
            out.push_str("&allow_insecure=1");
        }
    }
    if !node.name.is_empty() {
        out.push('#');
        out.push_str(&node.name);
    }
    Ok(out)
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p proxy-core --test protocol_tuic`
Expected: PASS（3 个测试）

- [ ] **Step 5: Commit**

```bash
git add crates/proxy-core/src/protocols/tuic.rs
git commit -m "feat(proxy-core): tuic protocol parse and serialize"
```

---

### Task 12: wireguard:// 协议解析与序列化

**Files:**
- Create: `crates/proxy-core/src/protocols/wireguard.rs`
- Test: `crates/proxy-core/tests/protocol_wireguard.rs`

**Interfaces:**
- Consumes: `ProxyNode`, `ParseError`, `SerializeError`, `uri::{percent_decode, parse_host_port}`
- Produces:
  - `pub fn is_wireguard(uri: &str) -> bool`
  - `pub fn parse_wireguard(uri: &str) -> Result<ProxyNode, ParseError>`
  - `pub fn serialize_wireguard(node: &ProxyNode) -> Result<String, SerializeError>`

**wireguard 格式规格（v2rayN/NeKoBox 常见）：**
```
wireguard://base64url(publicKey)@host:port?publicKey=...&privateKey=...&reserved=...&mtu=...&ip=10.0.0.1#name
```
实际广泛使用的格式把必要字段放在 query（`publicKey`、`privateKey`、`reserved`、`ip` 为逗号分隔 CIDR 或 IP 列表）。server 地址放 host:port。解析后 `uuid` 字段复用存放 privateKey（WireGuard 私钥即 32 字节 base64 密钥，语义相近），`password` 存放 publicKey。

- [ ] **Step 1: 写失败测试 `tests/protocol_wireguard.rs`**

```rust
use proxy_core::model::Protocol;
use proxy_core::protocols::wireguard::{parse_wireguard, serialize_wireguard};

const WG: &str = "wireguard://cHVibGljS2V5MTIz@1.2.3.4:443?publicKey=cHVibGljS2V5MTIz&privateKey=cHJpdmF0ZUtleTEyMw==&reserved=0,0,0&mtu=1420&ip=10.0.0.1%2F24,fd00::1%2F64#SG-01";

#[test]
fn parse_wireguard_basic() {
    let n = parse_wireguard(WG).unwrap();
    assert_eq!(n.kind, Protocol::Wireguard);
    assert_eq!(n.server, "1.2.3.4");
    assert_eq!(n.port, 443);
    assert_eq!(n.name, "SG-01");
    // uuid 字段存放 privateKey
    assert_eq!(n.uuid.as_deref(), Some("cHJpdmF0ZUtleTEyMw=="));
}

#[test]
fn wireguard_roundtrip() {
    let n = parse_wireguard(WG).unwrap();
    let out = serialize_wireguard(&n).unwrap();
    assert!(out.starts_with("wireguard://"));
    let n2 = parse_wireguard(&out).unwrap();
    assert_eq!(n2.uuid, n.uuid);
    assert_eq!(n2.server, n.server);
}

#[test]
fn wireguard_invalid() {
    assert!(parse_wireguard("wireguard://").is_err());
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p proxy-core --test protocol_wireguard`
Expected: FAIL — `cannot find function parse_wireguard`

- [ ] **Step 3: 实现 wireguard.rs**

```rust
// crates/proxy-core/src/protocols/wireguard.rs
use crate::error::{ParseError, SerializeError};
use crate::model::{Protocol, ProxyNode};
use crate::uri::{parse_host_port, percent_decode};
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
    let (hostpart, query) = match host_query.find('?') {
        Some(i) => (&host_query[..i], Some(&host_query[i + 1..])),
        None => (host_query, None),
    };
    let (server, port) = parse_host_port(hostpart)?;

    let mut public_key = None;
    let mut private_key = None;
    let mut ips = Vec::new();

    if let Some(q) = query {
        for kv in q.split('&') {
            let Some((k, v)) = kv.split_once('=') else { continue };
            let v = percent_decode(v).unwrap_or_default();
            match k {
                "publicKey" => public_key = Some(v),
                "privateKey" => private_key = Some(v),
                "ip" => ips = v.split(',').map(|s| s.to_string()).collect(),
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
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p proxy-core --test protocol_wireguard`
Expected: PASS（3 个测试）

- [ ] **Step 5: Commit**

```bash
git add crates/proxy-core/src/protocols/wireguard.rs
git commit -m "feat(proxy-core): wireguard protocol parse and serialize"
```

---

### Task 13: parser.rs 统一分发入口

**Files:**
- Create: `crates/proxy-core/src/parser.rs`
- Test: `crates/proxy-core/tests/parser_dispatch.rs`

**Interfaces:**
- Consumes: 全部协议模块的 `parse_*` + `is_*`
- Produces:
  - `pub fn parse_line(line: &str) -> Result<ProxyNode, ParseError>`
  - `pub fn parse_v2ray_subscription(text: &str, max_nodes: usize) -> (Vec<ProxyNode>, usize)` — 返回 `(成功节点, 跳过数)`，解析 base64 订阅文本
  - `pub fn parse_clash_yaml(text: &str) -> Result<Vec<ProxyNode>, ParseError>`
  - `pub fn parse_subscription_text(text: &str, max_nodes: usize) -> (Vec<ProxyNode>, usize)` — 自动识别 base64 vs 明文

- [ ] **Step 1: 写失败测试 `tests/parser_dispatch.rs`**

```rust
use proxy_core::model::Protocol;
use proxy_core::parser::{parse_clash_yaml, parse_line, parse_subscription_text, parse_v2ray_subscription};

#[test]
fn dispatch_by_prefix() {
    assert_eq!(parse_line("ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#T").unwrap().kind, Protocol::Ss);
    assert_eq!(parse_line("vmess://eyJ2IjoiMiJ9").unwrap().kind, Protocol::Vmess);
    assert!(parse_line("unknown://x").is_err());
}

#[test]
fn v2ray_subscription_base64() {
    // 两行 ss + 一行 vmess 的 base64
    let sub = "YWVzLTI1Ni1nY206cGFzcw@h:8388#A\nYWVzLTI1Ni1nY206cGFzcw@h:8389#B";
    // 注意：这里内容是 ss 行拼 base64 前的原文。测试里直接用明文多行。
    let _ = sub;
}

#[test]
fn subscription_plaintext_lines() {
    let text = "ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#A\nss://YWVzLTI1Ni1nY206cGFzcw@h:8389#B\nbad-line";
    let (nodes, skipped) = parse_subscription_text(text, 100);
    assert_eq!(nodes.len(), 2);
    assert_eq!(skipped, 1);
}

#[test]
fn subscription_max_nodes_limits() {
    let mut lines = String::new();
    for i in 0..10 {
        lines.push_str(&format!("ss://YWVzLTI1Ni1nY206cGFzcw@h:{}#N{}\n", 8000 + i, i));
    }
    let (nodes, _) = parse_subscription_text(&lines, 5);
    assert_eq!(nodes.len(), 5);
}

#[test]
fn clash_yaml_parsing() {
    let yaml = r#"
proxies:
  - name: "JP-01"
    type: trojan
    server: 1.2.3.4
    port: 443
    password: pass123
    sni: example.com
  - name: "US-01"
    type: vmess
    server: 5.6.7.8
    port: 443
    uuid: 11111111-2222-3333-4444-555555555555
    alterId: 0
    cipher: auto
    tls: true
    network: ws
    ws-opts:
      path: /ws
      headers:
        Host: cdn.example.com
"#;
    let nodes = parse_clash_yaml(yaml).unwrap();
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].kind, Protocol::Trojan);
    assert_eq!(nodes[0].name, "JP-01");
    assert_eq!(nodes[0].password.as_deref(), Some("pass123"));
    assert_eq!(nodes[1].kind, Protocol::Vmess);
    let ws = nodes[1].transport.as_ref().and_then(|t| t.websocket.as_ref()).unwrap();
    assert_eq!(ws.path, "/ws");
}
"#;
    let _ = yaml;
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p proxy-core --test parser_dispatch`
Expected: FAIL — `cannot find function parse_line`

- [ ] **Step 3: 实现 parser.rs**

```rust
// crates/proxy-core/src/parser.rs
use crate::error::ParseError;
use crate::model::ProxyNode;
use crate::protocols::{
    http, hysteria, hysteria2, ss, ssr, socks5, trojan, tuic, vless, vmess, wireguard,
};
use crate::uri::decode_base64_url_string;

/// 按协议前缀分发到各协议 parser。
pub fn parse_line(line: &str) -> Result<ProxyNode, ParseError> {
    let l = line.trim();
    if l.is_empty() {
        return Err(ParseError::UnsupportedProtocol);
    }
    if ss::is_ss(l) {
        ss::parse_ss(l)
    } else if ssr::is_ssr(l) {
        ssr::parse_ssr(l)
    } else if vmess::is_vmess(l) {
        vmess::parse_vmess(l)
    } else if vless::is_vless(l) {
        vless::parse_vless(l)
    } else if trojan::is_trojan(l) {
        trojan::parse_trojan(l)
    } else if socks5::is_socks5(l) {
        socks5::parse_socks5(l)
    } else if http::is_http(l) {
        http::parse_http(l)
    } else if hysteria2::is_hysteria2(l) {
        hysteria2::parse_hysteria2(l)
    } else if hysteria::is_hysteria(l) {
        hysteria::parse_hysteria(l)
    } else if tuic::is_tuic(l) {
        tuic::parse_tuic(l)
    } else if wireguard::is_wireguard(l) {
        wireguard::parse_wireguard(l)
    } else {
        Err(ParseError::UnsupportedProtocol)
    }
}

/// 解析 base64 订阅：先解码再逐行解析。
pub fn parse_v2ray_subscription(text: &str, max_nodes: usize) -> (Vec<ProxyNode>, usize) {
    // 若整个 body 是 base64，则先解码
    let body = decode_base64_url_string(text).unwrap_or_else(|_| text.to_string());
    parse_lines(&body, max_nodes)
}

fn parse_lines(text: &str, max_nodes: usize) -> (Vec<ProxyNode>, usize) {
    let mut nodes = Vec::new();
    let mut skipped = 0usize;
    for line in text.lines() {
        if nodes.len() >= max_nodes {
            break;
        }
        match parse_line(line) {
            Ok(n) => nodes.push(n),
            Err(_) => skipped += 1,
        }
    }
    (nodes, skipped)
}

/// 自动识别订阅文本：尝试 base64 解码，若成功且含换行则按解码结果解析；否则按明文。
pub fn parse_subscription_text(text: &str, max_nodes: usize) -> (Vec<ProxyNode>, usize) {
    // 若文本看起来是纯 base64（无协议前缀），尝试整体解码
    let trimmed = text.trim();
    let looks_base64 = !trimmed.contains("://") && trimmed.len() > 16;
    if looks_base64 {
        if let Ok(decoded) = decode_base64_url_string(trimmed) {
            if decoded.contains('\n') || decoded.contains("://") {
                return parse_lines(&decoded, max_nodes);
            }
        }
    }
    parse_lines(text, max_nodes)
}

/// 解析 Clash YAML 的 proxies 段。
pub fn parse_clash_yaml(text: &str) -> Result<Vec<ProxyNode>, ParseError> {
    let doc: serde_yaml::Value = serde_yaml::from_str(text).map_err(|e| ParseError::InvalidUri(e.to_string()))?;
    let Some(proxies) = doc.get("proxies").and_then(|p| p.as_sequence()) else {
        return Ok(Vec::new());
    };
    let mut nodes = Vec::new();
    for p in proxies {
        if let Some(n) = clash_proxy_to_node(p) {
            nodes.push(n);
        }
    }
    Ok(nodes)
}

fn clash_proxy_to_node(p: &serde_yaml::Value) -> Option<ProxyNode> {
    let name = p.get("name")?.as_str()?.to_string();
    let ty = p.get("type")?.as_str()?.to_lowercase();
    let server = p.get("server")?.as_str()?.to_string();
    let port: u16 = p.get("port")?.as_u64()?.try_into().ok()?;

    let mut node = ProxyNode {
        name,
        kind: crate::model::Protocol::from_str(&ty)?,
        server,
        port,
        ..Default::default()
    };
    // 各类型共性字段
    if let Some(s) = p.get("password").and_then(|v| v.as_str()) {
        node.password = Some(s.to_string());
    }
    if let Some(s) = p.get("uuid").and_then(|v| v.as_str()) {
        node.uuid = Some(s.to_string());
    }
    if let Some(alter_id) = p.get("alterId").or_else(|| p.get("alter-id")).and_then(|v| v.as_u64()) {
        node.alter_id = alter_id.try_into().ok();
    }
    // 加密
    if let Some(c) = p.get("cipher").or_else(|| p.get("method")).and_then(|v| v.as_str()) {
        if c != "auto" {
            node.crypto = Some(crate::model::Crypto::from_str(c));
        }
    }
    // TLS
    let tls_on = p.get("tls").and_then(|v| v.as_bool()).unwrap_or(false)
        || p.get("security").and_then(|v| v.as_str()).map(|s| s != "none").unwrap_or(false);
    if tls_on {
        node.tls = Some(crate::model::TlsSettings {
            enabled: true,
            sni: p.get("sni").and_then(|v| v.as_str()).map(|s| s.to_string())
                .or_else(|| p.get("servername").and_then(|v| v.as_str()).map(|s| s.to_string())),
            alpn: p.get("alpn").and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default(),
            insecure: p.get("skip-cert-verify").and_then(|v| v.as_bool()).unwrap_or(false),
            fingerprint: p.get("client-fingerprint").and_then(|v| v.as_str()).map(|s| s.to_string()),
        });
    }
    // 传输
    let net = p.get("network").and_then(|v| v.as_str()).unwrap_or("tcp");
    match net {
        "ws" | "websocket" => {
            let path = p.get("ws-opts").and_then(|o| o.get("path")).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let host = p.get("ws-opts").and_then(|o| o.get("headers")).and_then(|h| h.get("Host")).and_then(|v| v.as_str()).map(|s| s.to_string());
            node.transport = Some(crate::model::Transport {
                websocket: Some(crate::model::WebsocketConfig { path, host, headers: Default::default() }),
                ..Default::default()
            });
        }
        "grpc" => {
            let service = p.get("grpc-opts").and_then(|o| o.get("grpc-service-name")).and_then(|v| v.as_str()).unwrap_or("").to_string();
            node.transport = Some(crate::model::Transport {
                grpc: Some(crate::model::GrpcConfig { service_name: service }),
                ..Default::default()
            });
        }
        _ => {}
    }
    Some(node)
}
```

- [ ] **Step 4: 修正测试文件**（上一步测试里有两个拼错的占位字符串，删除无用的 `const _ = sub;` 和多余 `let _ = yaml;`，保留核心断言）

```rust
#[test]
fn v2ray_subscription_base64() {
    // 多行 ss 链接拼成 base64
    let plain = "ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#A\nss://YWVzLTI1Ni1nY206cGFzcw@h:8389#B";
    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, plain.as_bytes());
    let (nodes, _) = parse_v2ray_subscription(&encoded, 100);
    assert_eq!(nodes.len(), 2);
}
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test -p proxy-core --test parser_dispatch`
Expected: PASS（5 个测试）

> 注意：`crates/proxy-core/tests/parser_dispatch.rs` 需要 `base64` 依赖。已在 Cargo.toml（dev 依赖已有，主依赖也有）。

- [ ] **Step 6: Commit**

```bash
git add crates/proxy-core/src/parser.rs
git commit -m "feat(proxy-core): unified parser dispatch with defensive handling"
```

---

### Task 14: 输出序列化 —— Clash YAML、V2Ray 订阅、Sing-box JSON

**Files:**
- Create: `crates/proxy-core/src/serializer.rs`
- Create: `crates/proxy-core/src/formats/mod.rs`
- Create: `crates/proxy-core/src/formats/clash.rs`
- Create: `crates/proxy-core/src/formats/v2ray.rs`
- Create: `crates/proxy-core/src/formats/singbox.rs`
- Test: `crates/proxy-core/tests/formats.rs`

**Interfaces:**
- Consumes: `ProxyNode`, 各协议 `serialize_*`
- Produces:
  - `pub enum OutputFormat { Clash, V2ray, Singbox }` + `impl FromStr`
  - `pub fn serialize_nodes(nodes: &[ProxyNode], format: OutputFormat) -> Result<String, SerializeError>`
  - clash.rs: `pub fn serialize_clash(nodes: &[ProxyNode]) -> Result<String, SerializeError>`
  - v2ray.rs: `pub fn serialize_v2ray(nodes: &[ProxyNode]) -> Result<String, SerializeError>`
  - singbox.rs: `pub fn serialize_singbox(nodes: &[ProxyNode]) -> Result<String, SerializeError>`

- [ ] **Step 1: 写失败测试 `tests/formats.rs`**

```rust
use proxy_core::model::{Crypto, Protocol, ProxyNode};
use proxy_core::serializer::{serialize_nodes, OutputFormat};
use proxy_core::formats::clash::serialize_clash;
use proxy_core::formats::singbox::serialize_singbox;
use proxy_core::formats::v2ray::serialize_v2ray;

fn ss_node(name: &str, server: &str, port: u16) -> ProxyNode {
    ProxyNode {
        name: name.into(),
        kind: Protocol::Ss,
        server: server.into(),
        port,
        crypto: Some(Crypto::Aes256Gcm),
        password: Some("pass".into()),
        ..Default::default()
    }
}

fn trojan_node(name: &str, server: &str, port: u16) -> ProxyNode {
    ProxyNode {
        name: name.into(),
        kind: Protocol::Trojan,
        server: server.into(),
        port,
        password: Some("pw".into()),
        tls: Some(proxy_core::model::TlsSettings {
            enabled: true,
            sni: Some(server.into()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn clash_yaml_has_proxies_and_groups() {
    let nodes = vec![ss_node("A", "1.2.3.4", 8388), trojan_node("B", "5.6.7.8", 443)];
    let out = serialize_clash(&nodes).unwrap();
    assert!(out.contains("proxies:"));
    assert!(out.contains("proxy-groups:"));
    assert!(out.contains("name: A"));
    assert!(out.contains("type: ss"));
    assert!(out.contains("type: trojan"));
}

#[test]
fn v2ray_subscription_uri_lines() {
    let nodes = vec![ss_node("A", "1.2.3.4", 8388)];
    let out = serialize_v2ray(&nodes).unwrap();
    // 是 base64，解码后含 ss://
    let decoded = proxy_core::uri::decode_base64_url_string(&out).unwrap();
    assert!(decoded.contains("ss://"));
    assert!(decoded.contains("1.2.3.4:8388"));
}

#[test]
fn singbox_json_outbounds() {
    let nodes = vec![ss_node("A", "1.2.3.4", 8388), trojan_node("B", "5.6.7.8", 443)];
    let out = serialize_singbox(&nodes).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let outbounds = v["outbounds"].as_array().unwrap();
    assert_eq!(outbounds.len(), 2);
    assert_eq!(outbounds[0]["type"], "shadowsocks");
    assert_eq!(outbounds[1]["type"], "trojan");
}

#[test]
fn serialize_dispatch() {
    let nodes = vec![ss_node("A", "1.2.3.4", 8388)];
    let clash = serialize_nodes(&nodes, OutputFormat::Clash).unwrap();
    assert!(clash.contains("proxies:"));
    let v2 = serialize_nodes(&nodes, OutputFormat::V2ray).unwrap();
    assert!(v2.contains("ss://") || v2.len() > 20);
    let sb = serialize_nodes(&nodes, OutputFormat::Singbox).unwrap();
    assert!(sb.contains("outbounds"));
}

#[test]
fn empty_nodes_ok() {
    assert!(serialize_clash(&[]).is_ok());
    assert!(serialize_singbox(&[]).is_ok());
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p proxy-core --test formats`
Expected: FAIL — `cannot find function serialize_clash`

- [ ] **Step 3: 实现 serializer.rs（分发入口）**

```rust
// crates/proxy-core/src/serializer.rs
use crate::error::SerializeError;
use crate::formats::{clash, singbox, v2ray};
use crate::model::ProxyNode;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Clash,
    V2ray,
    Singbox,
}

impl FromStr for OutputFormat {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        match s.to_ascii_lowercase().as_str() {
            "clash" | "clashyaml" | "yaml" => Ok(Self::Clash),
            "v2ray" | "v2r" | "base64" => Ok(Self::V2ray),
            "singbox" | "sing-box" | "json" => Ok(Self::Singbox),
            _ => Err(()),
        }
    }
}

pub fn serialize_nodes(nodes: &[ProxyNode], format: OutputFormat) -> Result<String, SerializeError> {
    match format {
        OutputFormat::Clash => clash::serialize_clash(nodes),
        OutputFormat::V2ray => v2ray::serialize_v2ray(nodes),
        OutputFormat::Singbox => singbox::serialize_singbox(nodes),
    }
}
```

- [ ] **Step 4: 实现 formats/mod.rs**

```rust
// crates/proxy-core/src/formats/mod.rs
pub mod clash;
pub mod singbox;
pub mod v2ray;
```

- [ ] **Step 5: 实现 formats/clash.rs**

```rust
// crates/proxy-core/src/formats/clash.rs
use crate::error::SerializeError;
use crate::model::{Protocol, ProxyNode};

pub fn serialize_clash(nodes: &[ProxyNode]) -> Result<String, SerializeError> {
    let mut out = String::from("mixed-port: 7890\nallow-lan: false\nmode: rule\nlog-level: info\n\n");
    out.push_str("proxies:\n");
    for n in nodes {
        out.push_str(&proxy_to_clash(n)?);
    }
    if !nodes.is_empty() {
        out.push('\n');
        out.push_str("proxy-groups:\n");
        out.push_str("  - name: \"🚀 节点选择\"\n    type: select\n    proxies:\n");
        for n in nodes {
            out.push_str(&format!("      - {}\n", clash_yaml_str(&n.name)));
        }
        out.push_str("      - DIRECT\n");
        out.push_str("  - name: \"♻️ 自动选择\"\n    type: url-test\n    url: http://www.gstatic.com/generate_204\n    interval: 300\n    proxies:\n");
        for n in nodes {
            out.push_str(&format!("      - {}\n", clash_yaml_str(&n.name)));
        }
        out.push_str("\nrules:\n  - MATCH,🚀 节点选择\n");
    }
    Ok(out)
}

fn clash_yaml_str(s: &str) -> String {
    if s.chars().any(|c| c.is_whitespace() || c == ':' || c == '#' || c == ',' || c == '[' || c == ']') {
        format!("\"{}\"", s.replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

fn proxy_to_clash(n: &ProxyNode) -> Result<String, SerializeError> {
    let mut s = format!("  - name: {}\n    type: {}\n", clash_yaml_str(&n.name), clash_type(n.kind));
    s.push_str(&format!("    server: {}\n    port: {}\n", n.server, n.port));
    match &n.kind {
        Protocol::Ss => {
            let c = n.crypto.as_ref().ok_or(SerializeError::MissingField("crypto"))?;
            let p = n.password.as_ref().ok_or(SerializeError::MissingField("password"))?;
            s.push_str(&format!("    cipher: {}\n    password: {}\n", clash_yaml_str(c.as_str()), clash_yaml_str(p)));
        }
        Protocol::Ssr => {
            let c = n.crypto.as_ref().ok_or(SerializeError::MissingField("crypto"))?;
            let p = n.password.as_ref().ok_or(SerializeError::MissingField("password"))?;
            s.push_str(&format!("    cipher: {}\n    password: {}\n", clash_yaml_str(c.as_str()), clash_yaml_str(p)));
        }
        Protocol::Vmess => {
            let u = n.uuid.as_ref().ok_or(SerializeError::MissingField("uuid"))?;
            s.push_str(&format!("    uuid: {}\n    alterId: {}\n    cipher: auto\n", clash_yaml_str(u), n.alter_id.unwrap_or(0)));
        }
        Protocol::Vless => {
            let u = n.uuid.as_ref().ok_or(SerializeError::MissingField("uuid"))?;
            s.push_str(&format!("    uuid: {}\n", clash_yaml_str(u)));
        }
        Protocol::Trojan => {
            let p = n.password.as_ref().ok_or(SerializeError::MissingField("password"))?;
            s.push_str(&format!("    password: {}\n", clash_yaml_str(p)));
        }
        Protocol::Hysteria | Protocol::Hysteria2 | Protocol::Tuic => {
            if let Some(p) = &n.password {
                s.push_str(&format!("    password: {}\n", clash_yaml_str(p)));
            }
            if let Some(u) = &n.uuid {
                s.push_str(&format!("    uuid: {}\n", clash_yaml_str(u)));
            }
        }
        Protocol::Socks5 => {
            if let Some(p) = &n.password {
                s.push_str(&format!("    username: {}\n    password: {}\n", clash_yaml_str(&n.server), clash_yaml_str(p)));
            }
        }
        Protocol::Http => {
            if let Some(p) = &n.password {
                s.push_str(&format!("    username: {}\n    password: {}\n", clash_yaml_str(&n.server), clash_yaml_str(p)));
            }
        }
        Protocol::Wireguard => {
            let pubk = n.password.as_ref().ok_or(SerializeError::MissingField("publicKey"))?;
            let privk = n.uuid.as_ref().ok_or(SerializeError::MissingField("privateKey"))?;
            s.push_str(&format!("    public-key: {}\n    private-key: {}\n", clash_yaml_str(pubk), clash_yaml_str(privk)));
        }
    }
    if let Some(t) = &n.tls {
        if t.enabled {
            s.push_str("    tls: true\n");
        }
        if let Some(sni) = &t.sni {
            s.push_str(&format!("    sni: {}\n", clash_yaml_str(sni)));
        }
        if !t.alpn.is_empty() {
            let alpn = t.alpn.iter().map(|a| format!("\"{}\"", a)).collect::<Vec<_>>().join(", ");
            s.push_str(&format!("    alpn:\n      - {}\n", t.alpn.iter().map(|a| a.to_string()).collect::<Vec<_>>().join("\n      - ")));
            let _ = alpn;
        }
        if t.insecure {
            s.push_str("    skip-cert-verify: true\n");
        }
        if let Some(fp) = &t.fingerprint {
            s.push_str(&format!("    client-fingerprint: {}\n", clash_yaml_str(fp)));
        }
    }
    if let Some(tr) = &n.transport {
        if let Some(ws) = &tr.websocket {
            s.push_str("    network: ws\n    ws-opts:\n");
            s.push_str(&format!("      path: {}\n", clash_yaml_str(&ws.path)));
            if let Some(h) = &ws.host {
                s.push_str(&format!("      headers:\n        Host: {}\n", clash_yaml_str(h)));
            }
        } else if let Some(g) = &tr.grpc {
            s.push_str("    network: grpc\n    grpc-opts:\n");
            s.push_str(&format!("      grpc-service-name: {}\n", clash_yaml_str(&g.service_name)));
        } else if let Some(h) = &tr.http_upgrade {
            s.push_str("    network: http\n    http-opts:\n");
            s.push_str(&format!("      path: {}\n", clash_yaml_str(&h.path)));
        }
    }
    Ok(s)
}

fn clash_type(k: Protocol) -> &'static str {
    match k {
        Protocol::Ss => "ss",
        Protocol::Ssr => "ssr",
        Protocol::Socks5 => "socks5",
        Protocol::Http => "http",
        Protocol::Vmess => "vmess",
        Protocol::Vless => "vless",
        Protocol::Trojan => "trojan",
        Protocol::Hysteria => "hysteria",
        Protocol::Hysteria2 => "hysteria2",
        Protocol::Tuic => "tuic",
        Protocol::Wireguard => "wireguard",
    }
}
```

- [ ] **Step 6: 实现 formats/v2ray.rs**

```rust
// crates/proxy-core/src/formats/v2ray.rs
use crate::error::SerializeError;
use crate::model::ProxyNode;
use crate::protocols::{ss, ssr, trojan, tuic, vless, vmess};

pub fn serialize_v2ray(nodes: &[ProxyNode]) -> Result<String, SerializeError> {
    let mut lines = Vec::new();
    for n in nodes {
        let line = match &n.kind {
            crate::model::Protocol::Ss => ss::serialize_ss(n),
            crate::model::Protocol::Ssr => ssr::serialize_ssr(n),
            crate::model::Protocol::Vmess => vmess::serialize_vmess(n),
            crate::model::Protocol::Vless => vless::serialize_vless(n),
            crate::model::Protocol::Trojan => trojan::serialize_trojan(n),
            crate::model::Protocol::Tuic => tuic::serialize_tuic(n),
            crate::model::Protocol::Hysteria | crate::model::Protocol::Hysteria2 => {
                // hysteria 系列的 URI 不在 V2Ray base64 订阅内；跳过（走单独格式）
                continue;
            }
            crate::model::Protocol::Wireguard => continue,
            crate::model::Protocol::Socks5 | crate::model::Protocol::Http => continue,
        }?;
        lines.push(line);
    }
    let joined = lines.join("\n");
    if joined.is_empty() {
        return Ok(String::new());
    }
    let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, joined.as_bytes());
    Ok(b64)
}
```

- [ ] **Step 7: 实现 formats/singbox.rs**

```rust
// crates/proxy-core/src/formats/singbox.rs
use crate::error::SerializeError;
use crate::model::{Protocol, ProxyNode};
use serde_json::json;

pub fn serialize_singbox(nodes: &[ProxyNode]) -> Result<String, SerializeError> {
    let outbounds: Vec<serde_json::Value> = nodes
        .iter()
        .filter_map(|n| node_to_singbox(n).ok())
        .collect();
    let v = json!({ "outbounds": outbounds });
    serde_json::to_string_pretty(&v).map_err(|_| SerializeError::MissingField("json"))
}

fn node_to_singbox(n: &ProxyNode) -> Result<serde_json::Value, SerializeError> {
    let tag = if n.name.is_empty() { &n.server } else { &n.name };
    let base = json!({
        "tag": tag,
        "server": n.server,
        "server_port": n.port,
    });
    let mut o = base;
    match &n.kind {
        Protocol::Ss => {
            let c = n.crypto.as_ref().ok_or(SerializeError::MissingField("crypto"))?;
            let p = n.password.as_ref().ok_or(SerializeError::MissingField("password"))?;
            o["type"] = json!("shadowsocks");
            o["method"] = json!(c.as_str());
            o["password"] = json!(p);
        }
        Protocol::Ssr => {
            let c = n.crypto.as_ref().ok_or(SerializeError::MissingField("crypto"))?;
            let p = n.password.as_ref().ok_or(SerializeError::MissingField("password"))?;
            o["type"] = json!("shadowsocksr");
            o["method"] = json!(c.as_str());
            o["password"] = json!(p);
            o["obfs"] = json!("plain");
            o["protocol"] = json!("auth_aes128_md5");
        }
        Protocol::Socks5 => {
            o["type"] = json!("socks");
            if let Some(p) = &n.password {
                o["username"] = json!(&n.server);
                o["password"] = json!(p);
            }
        }
        Protocol::Http => {
            o["type"] = json!("http");
            if let Some(p) = &n.password {
                o["username"] = json!(&n.server);
                o["password"] = json!(p);
            }
        }
        Protocol::Vmess => {
            let u = n.uuid.as_ref().ok_or(SerializeError::MissingField("uuid"))?;
            o["type"] = json!("vmess");
            o["uuid"] = json!(u);
            o["alter_id"] = json!(n.alter_id.unwrap_or(0));
        }
        Protocol::Vless => {
            let u = n.uuid.as_ref().ok_or(SerializeError::MissingField("uuid"))?;
            o["type"] = json!("vless");
            o["uuid"] = json!(u);
        }
        Protocol::Trojan => {
            let p = n.password.as_ref().ok_or(SerializeError::MissingField("password"))?;
            o["type"] = json!("trojan");
            o["password"] = json!(p);
        }
        Protocol::Hysteria => {
            let p = n.password.as_deref().unwrap_or_default();
            o["type"] = json!("hysteria");
            o["auth_str"] = json!(p);
            o["up_mbps"] = json!(100);
            o["down_mbps"] = json!(100);
        }
        Protocol::Hysteria2 => {
            o["type"] = json!("hysteria2");
            if let Some(p) = &n.password {
                o["password"] = json!(p);
            }
        }
        Protocol::Tuic => {
            o["type"] = json!("tuic");
            if let Some(u) = &n.uuid {
                o["uuid"] = json!(u);
            }
            if let Some(p) = &n.password {
                o["password"] = json!(p);
            }
            o["congestion_control"] = json!("bbr");
        }
        Protocol::Wireguard => {
            let pubk = n.password.as_ref().ok_or(SerializeError::MissingField("publicKey"))?;
            let privk = n.uuid.as_ref().ok_or(SerializeError::MissingField("privateKey"))?;
            o["type"] = json!("wireguard");
            o["public_key"] = json!(pubk);
            o["private_key"] = json!(privk);
        }
    }
    if let Some(t) = &n.tls {
        if t.enabled {
            o["tls"] = json!({ "enabled": true });
            if let Some(s) = &t.sni {
                o["tls"]["server_name"] = json!(s);
            }
            if !t.alpn.is_empty() {
                o["tls"]["alpn"] = json!(t.alpn);
            }
            if t.insecure {
                o["tls"]["insecure"] = json!(true);
            }
        }
    }
    if let Some(tr) = &n.transport {
        if let Some(ws) = &tr.websocket {
            let mut t = json!({ "type": "ws", "path": ws.path });
            if let Some(h) = &ws.host {
                t["headers"] = json!({ "Host": h });
            }
            o["transport"] = t;
        } else if let Some(g) = &tr.grpc {
            o["transport"] = json!({ "type": "grpc", "service_name": g.service_name });
        }
    }
    Ok(o)
}
```

- [ ] **Step 8: 运行测试确认通过**

Run: `cargo test -p proxy-core --test formats`
Expected: PASS（5 个测试）

- [ ] **Step 9: Commit**

```bash
git add crates/proxy-core/src/serializer.rs crates/proxy-core/src/formats/
git commit -m "feat(proxy-core): clash/v2ray/singbox output serializers"
```

---

### Task 15: 全协议往返测试

**Files:**
- Create: `crates/proxy-core/tests/roundtrip.rs`

**Interfaces:**
- Consumes: 所有 `parse_*` 与 `serialize_*`

- [ ] **Step 1: 写往返测试**

```rust
use proxy_core::model::Protocol;
use proxy_core::protocols::ss::{parse_ss, serialize_ss};
use proxy_core::protocols::ssr::{parse_ssr, serialize_ssr};
use proxy_core::protocols::vless::{parse_vless, serialize_vless};
use proxy_core::protocols::vmess::{parse_vmess, serialize_vmess};
use proxy_core::protocols::trojan::{parse_trojan, serialize_trojan};
use proxy_core::protocols::hysteria::{parse_hysteria, serialize_hysteria};
use proxy_core::protocols::hysteria2::{parse_hysteria2, serialize_hysteria2};
use proxy_core::protocols::tuic::{parse_tuic, serialize_tuic};
use proxy_core::protocols::wireguard::{parse_wireguard, serialize_wireguard};

/// 对每个协议的 (解析, 序列化) 对做往返：解析→序列化→再解析，关键字段不变。
macro_rules! roundtrip {
    ($uri:expr, $parser:ident, $serializer:ident, $kind:expr) => {{
        let n1 = $parser($uri).unwrap();
        assert_eq!(n1.kind, $kind);
        let out = $serializer(&n1).unwrap();
        let n2 = $parser(&out).unwrap();
        assert_eq!(n2.server, n1.server, "server changed for {}", $uri);
        assert_eq!(n2.port, n1.port, "port changed for {}", $uri);
    }};
}

#[test]
fn all_protocols_roundtrip() {
    roundtrip!(
        "ss://YWVzLTI1Ni1nY206cGFzc3dvcmQ@example.com:8388#US-01",
        parse_ss, serialize_ss, Protocol::Ss
    );
    roundtrip!(
        "ssr://MS4yLjMuNDo4Mzg4OmF1dGhfYWVzMTI4X21kNTphZXMtMjU2LWNmYjpwbGFpbjpwYXNzLz9vYnZzcGFyYW09JmlkPTE&remarks=VVMtMDE",
        parse_ssr, serialize_ssr, Protocol::Ssr
    );
    roundtrip!(
        "vmess://eyJ2IjoiMiIsInBzIjoiU0ctMDEiLCJhZGQiOiIxLjIuMy40IiwicG9ydCI6IjQ0MyIsImlkIjoidXVpZC0xMTExLTExMTEiLCJhaWQiOiIwIiwibmV0Ijoid3MiLCJ0eXBlIjoibm9uZSIsImhvc3QiOiJjZG4uZXhhbXBsZS5jb20iLCJwYXRoIjoiL3dzIiwidGxzIjoidGxzIn0=",
        parse_vmess, serialize_vmess, Protocol::Vmess
    );
    roundtrip!(
        "vless://11111111-2222-3333-4444-555555555555@1.2.3.4:443?encryption=none&security=tls&sni=cdn.example.com&type=ws&path=%2Fws&fp=chrome#JP-01",
        parse_vless, serialize_vless, Protocol::Vless
    );
    roundtrip!(
        "trojan://pass%40word@1.2.3.4:443?security=tls&sni=example.com#KR-01",
        parse_trojan, serialize_trojan, Protocol::Trojan
    );
    roundtrip!(
        "hysteria://1.2.3.4:36712?protocol=udp&auth=secret123&upmbps=100&downmbps=100&obfs=xorsalsa20&sni=example.com&insecure=1&alpn=hysteria#JP-01",
        parse_hysteria, serialize_hysteria, Protocol::Hysteria
    );
    roundtrip!(
        "hysteria2://pass%40word@1.2.3.4:8443?sni=example.com&insecure=0#US-02",
        parse_hysteria2, serialize_hysteria2, Protocol::Hysteria2
    );
    roundtrip!(
        "tuic://11111111-2222-3333-4444-555555555555:pass%40word@1.2.3.4:443?congestion_control=bbr&udp_relay_mode=native&alpn=h3&sni=example.com&allow_insecure=1#TW-01",
        parse_tuic, serialize_tuic, Protocol::Tuic
    );
    roundtrip!(
        "wireguard://cHVibGljS2V5MTIz@1.2.3.4:443?publicKey=cHVibGljS2V5MTIz&privateKey=cHJpdmF0ZUtleTEyMw==&reserved=0,0,0&mtu=1420&ip=10.0.0.1%2F24#SG-01",
        parse_wireguard, serialize_wireguard, Protocol::Wireguard
    );
}

#[test]
fn full_pipeline_parse_merge_serialize() {
    use proxy_core::parser::parse_subscription_text;
    use proxy_core::serializer::{serialize_nodes, OutputFormat};

    let sub = "ss://YWVzLTI1Ni1nY206cGFzc3dvcmQ@example.com:8388#A\nvmess://eyJ2IjoiMiIsInBzIjoiQiIsImFkZCI6IjUuNi43LjgiLCJwb3J0IjoiNDQzIiwiaWQiOiJ1IiwiYWlkIjoiMCIsIm5ldCI6InRjcCIsInRscyI6Im5vbmUifQ==";
    let (nodes, skipped) = parse_subscription_text(sub, 1000);
    assert_eq!(nodes.len(), 2);
    assert_eq!(skipped, 0);

    let clash = serialize_nodes(&nodes, OutputFormat::Clash).unwrap();
    assert!(clash.contains("proxies:"));
    assert!(clash.contains("name: A"));
    assert!(clash.contains("name: B"));
}
```

- [ ] **Step 2: 运行确认通过**

Run: `cargo test -p proxy-core --test roundtrip`
Expected: PASS（2 个测试，覆盖 9 种协议往返）

- [ ] **Step 3: 全量测试**

Run: `cargo test -p proxy-core`
Expected: 全部 PASS

- [ ] **Step 4: Commit**

```bash
git add crates/proxy-core/tests/roundtrip.rs
git commit -m "test(proxy-core): full protocol roundtrip and pipeline tests"
```

---

### Task 16: 解析器模糊测试（可选增强）

**Files:**
- Create: `crates/proxy-core/tests/proptest_fuzz.rs`
- 修改: `crates/proxy-core/Cargo.toml`（dev-dependencies 加 `proptest`）

**Interfaces:**
- Consumes: `parse_line`

- [ ] **Step 1: 加 proptest dev 依赖**

```toml
[dev-dependencies]
proptest = "1"
```

- [ ] **Step 2: 写模糊测试**

```rust
use proptest::prelude::*;
use proxy_core::parser::parse_line;

proptest! {
    #[test]
    fn parse_line_never_panics(s in ".*") {
        // 任意字符串输入，解析器必须返回 Result 而非 panic
        let _ = parse_line(&s);
    }

    #[test]
    fn parse_line_base64_mutations(base in "[a-zA-Z0-9+/=_-]{0,200}") {
        let _ = parse_line(&format!("vmess://{}", base));
        let _ = parse_line(&format!("ssr://{}", base));
        let _ = parse_line(&format!("ss://{}", base));
    }
}
```

- [ ] **Step 3: 运行确认通过**

Run: `cargo test -p proxy-core --test proptest_fuzz`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/proxy-core/tests/proptest_fuzz.rs crates/proxy-core/Cargo.toml
git commit -m "test(proxy-core): proptest fuzzing for parsers"
```

---

## Plan A 完成标准

- [ ] `cargo test -p proxy-core` 全绿
- [ ] 9 种协议 URI 解析/序列化 + socks5/http + 往返测试通过
- [ ] Clash / V2Ray / Sing-box 三种输出序列化通过
- [ ] `parse_line` 对任意输入不 panic（proptest）
- [ ] 全部 commit 已就位
