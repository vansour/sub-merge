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

    // trojan 协议强制 TLS 承载：仅当 security=none 显式出现时关闭 TLS，
    // 与 parse_clash_yaml 中 trojan 始终 TLS 的语义对齐。
    let tls = if security == "none" {
        None
    } else {
        Some(TlsSettings {
            enabled: true,
            sni: sni.or(host.clone()),
            alpn,
            insecure,
            fingerprint: fp,
        })
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
            ("httpupgrade", h.host.clone().unwrap_or_default(), h.path.clone())
        }
        _ => ("tcp", String::new(), String::new()),
    };
    // 查询参数条件组装：任一参数存在时先写 '?'，参数间用 '&' 连接
    let mut query: Vec<String> = Vec::new();
    if let Some(t) = &node.tls {
        if t.enabled {
            query.push("security=tls".into());
        }
        if let Some(s) = &t.sni {
            query.push(format!("sni={}", encode(s)));
        }
        if let Some(fp) = &t.fingerprint {
            query.push(format!("fp={}", encode(fp)));
        }
        if !t.alpn.is_empty() {
            query.push(format!("alpn={}", encode(&t.alpn.join(","))));
        }
    }
    if net != "tcp" {
        query.push(format!("type={}", net));
    }
    if !host.is_empty() {
        query.push(format!("host={}", encode(&host)));
    }
    if !path.is_empty() {
        query.push(format!("path={}", encode(&path)));
    }
    if !query.is_empty() {
        out.push('?');
        out.push_str(&query.join("&"));
    }
    if !node.name.is_empty() {
        out.push('#');
        out.push_str(&node.name);
    }
    Ok(out)
}
