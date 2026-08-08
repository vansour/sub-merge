// crates/proxy-core/src/lib.rs
pub mod error;
pub mod formats;
pub mod model;
pub mod parser;
pub mod protocols;
pub mod uri;

pub use error::{ParseError, SerializeError};
pub use model::{
    Crypto, GrpcConfig, HttpUpgradeConfig, Protocol, ProxyNode, TlsSettings, Transport,
    WebsocketConfig,
};
