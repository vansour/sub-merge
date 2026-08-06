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
    let tag = if n.name.is_empty() {
        &n.server
    } else {
        &n.name
    };
    let base = json!({
        "tag": tag,
        "server": n.server,
        "server_port": n.port,
    });
    let mut o = base;
    match &n.kind {
        Protocol::Ss => {
            let c = n
                .crypto
                .as_ref()
                .ok_or(SerializeError::MissingField("crypto"))?;
            let p = n
                .password
                .as_ref()
                .ok_or(SerializeError::MissingField("password"))?;
            o["type"] = json!("shadowsocks");
            o["method"] = json!(c.as_str());
            o["password"] = json!(p);
        }
        Protocol::Ssr => {
            let c = n
                .crypto
                .as_ref()
                .ok_or(SerializeError::MissingField("crypto"))?;
            let p = n
                .password
                .as_ref()
                .ok_or(SerializeError::MissingField("password"))?;
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
            let u = n
                .uuid
                .as_ref()
                .ok_or(SerializeError::MissingField("uuid"))?;
            o["type"] = json!("vmess");
            o["uuid"] = json!(u);
            o["alter_id"] = json!(n.alter_id.unwrap_or(0));
        }
        Protocol::Vless => {
            let u = n
                .uuid
                .as_ref()
                .ok_or(SerializeError::MissingField("uuid"))?;
            o["type"] = json!("vless");
            o["uuid"] = json!(u);
        }
        Protocol::Trojan => {
            let p = n
                .password
                .as_ref()
                .ok_or(SerializeError::MissingField("password"))?;
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
            let pubk = n
                .password
                .as_ref()
                .ok_or(SerializeError::MissingField("publicKey"))?;
            let privk = n
                .uuid
                .as_ref()
                .ok_or(SerializeError::MissingField("privateKey"))?;
            o["type"] = json!("wireguard");
            o["public_key"] = json!(pubk);
            o["private_key"] = json!(privk);
        }
    }
    if let Some(t) = &n.tls
        && t.enabled
    {
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
