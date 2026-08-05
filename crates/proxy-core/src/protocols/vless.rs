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
            ("httpupgrade", h.host.clone().unwrap_or_default(), h.path.clone())
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
