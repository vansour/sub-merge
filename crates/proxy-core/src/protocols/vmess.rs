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
    let payload = uri
        .strip_prefix("vmess://")
        .ok_or(ParseError::UnsupportedProtocol)?;
    let json_str = decode_base64_url_string(payload)?;
    let v: serde_json::Value =
        serde_json::from_str(&json_str).map_err(|_| ParseError::InvalidUri(uri.to_string()))?;

    let get = |k: &str| -> Option<String> {
        v.get(k).and_then(|x| match x {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            serde_json::Value::Bool(b) => Some(b.to_string()),
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
    let tls_enabled = matches!(
        get("tls").as_deref(),
        Some("tls") | Some("xtls") | Some("reality")
    ) || get("security").as_deref() == Some("tls");
    let host = get("host");
    let path = get("path").unwrap_or_default();
    let sni = get("sni").filter(|s| !s.is_empty()).or(host.clone());
    let alpn = get("alpn")
        .map(|a| {
            a.split(',')
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let insecure = get("allowInsecure")
        .map(|s| s == "1" || s == "true")
        .unwrap_or(false);
    let fp = get("fp").filter(|s| !s.is_empty());

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
                host,
                headers: Default::default(),
            }),
            ..Default::default()
        }),
        "grpc" => Some(Transport {
            grpc: Some(GrpcConfig {
                service_name: path.trim_start_matches('/').to_string(),
            }),
            ..Default::default()
        }),
        "httpupgrade" | "http" => Some(Transport {
            http_upgrade: Some(HttpUpgradeConfig { path, host }),
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
    let (net, host, path, type_): (&str, String, String, &str) = match &node.transport {
        Some(t) if t.websocket.is_some() => {
            let ws = t.websocket.as_ref().unwrap();
            (
                "ws",
                ws.host.clone().unwrap_or_default(),
                ws.path.clone(),
                "none",
            )
        }
        Some(t) if t.grpc.is_some() => {
            let g = t.grpc.as_ref().unwrap();
            (
                "grpc",
                String::new(),
                format!("/{}", g.service_name),
                "none",
            )
        }
        Some(t) if t.http_upgrade.is_some() => {
            let h = t.http_upgrade.as_ref().unwrap();
            (
                "http",
                h.host.clone().unwrap_or_default(),
                h.path.clone(),
                "none",
            )
        }
        _ => ("tcp", String::new(), String::new(), "none"),
    };

    let tls_str = if node.tls.as_ref().map(|t| t.enabled).unwrap_or(false) {
        "tls"
    } else {
        "none"
    };
    let sni = node
        .tls
        .as_ref()
        .and_then(|t| t.sni.clone())
        .unwrap_or_default();
    let alpn = node
        .tls
        .as_ref()
        .map(|t| t.alpn.join(","))
        .unwrap_or_default();
    let fp = node
        .tls
        .as_ref()
        .and_then(|t| t.fingerprint.clone())
        .unwrap_or_default();
    let insecure = node.tls.as_ref().map(|t| t.insecure).unwrap_or(false);

    // 空的可选字段（host/sni/alpn/fp）不写入 JSON，保证 parse->serialize 幂等。
    let mut obj = serde_json::Map::new();
    obj.insert("v".into(), json!("2"));
    obj.insert("ps".into(), json!(node.name));
    obj.insert("add".into(), json!(node.server));
    obj.insert("port".into(), json!(node.port.to_string()));
    obj.insert("id".into(), json!(uuid));
    obj.insert("aid".into(), json!(node.alter_id.unwrap_or(0).to_string()));
    obj.insert("net".into(), json!(net));
    obj.insert("type".into(), json!(type_));
    obj.insert("path".into(), json!(path));
    if !host.is_empty() {
        obj.insert("host".into(), json!(host));
    }
    obj.insert("tls".into(), json!(tls_str));
    if !sni.is_empty() {
        obj.insert("sni".into(), json!(sni));
    }
    if !alpn.is_empty() {
        obj.insert("alpn".into(), json!(alpn));
    }
    if !fp.is_empty() {
        obj.insert("fp".into(), json!(fp));
    }
    obj.insert(
        "allowInsecure".into(),
        json!(if insecure { "1" } else { "0" }),
    );
    let s = serde_json::to_string(&serde_json::Value::Object(obj))
        .map_err(|_| SerializeError::MissingField("json"))?;
    let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, s.as_bytes());
    Ok(format!("vmess://{}", b64))
}
