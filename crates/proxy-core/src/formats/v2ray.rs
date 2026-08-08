// crates/proxy-core/src/formats/v2ray.rs
use crate::error::SerializeError;
use crate::model::ProxyNode;
use crate::protocols::{ss, ssr, trojan, tuic, vless, vmess};

/// 纯 URI 文本行输出（每行一个节点，无 base64 包裹）。空节点列表返回空串。
pub fn serialize_v2ray_plain(nodes: &[ProxyNode]) -> Result<String, SerializeError> {
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
    Ok(lines.join("\n"))
}

pub fn serialize_v2ray(nodes: &[ProxyNode]) -> Result<String, SerializeError> {
    let joined = serialize_v2ray_plain(nodes)?;
    if joined.is_empty() {
        return Ok(String::new());
    }
    let b64 = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        joined.as_bytes(),
    );
    Ok(b64)
}
