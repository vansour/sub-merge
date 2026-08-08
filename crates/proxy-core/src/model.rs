// crates/proxy-core/src/model.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    #[default]
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

    // 返回 Option 的宽松解析（含别名容错），不符合 std::str::FromStr 的 Result 形态
    #[allow(clippy::should_implement_trait)]
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
    // 永不失败的兜底解析（未知值走 Raw），不符合 std::str::FromStr 的 Result 形态
    #[allow(clippy::should_implement_trait)]
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
    /// 流控（vless reality 常用 xtls-rprx-vision；vmess/vless 均可能携带）
    pub flow: Option<String>,
    /// reality 公钥（pbk）：reality 握手必需，缺失则节点无法连接
    pub pbk: Option<String>,
    /// reality shortId（sid），可空串
    pub sid: Option<String>,
    /// reality spiderX（spx）
    pub spx: Option<String>,
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
