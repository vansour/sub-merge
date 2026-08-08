// crates/proxy-core/src/serializer.rs
use crate::error::SerializeError;
use crate::formats::{clash, v2ray};
use crate::model::ProxyNode;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Clash,
    V2ray,
}

impl FromStr for OutputFormat {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        match s.to_ascii_lowercase().as_str() {
            "clash" | "clashyaml" | "yaml" => Ok(Self::Clash),
            "v2ray" | "v2r" | "base64" => Ok(Self::V2ray),
            _ => Err(()),
        }
    }
}

pub fn serialize_nodes(
    nodes: &[ProxyNode],
    format: OutputFormat,
) -> Result<String, SerializeError> {
    match format {
        OutputFormat::Clash => clash::serialize_clash(nodes),
        OutputFormat::V2ray => v2ray::serialize_v2ray(nodes),
    }
}
