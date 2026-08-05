// crates/proxy-core/src/lib.rs
// Task 1 只声明本任务创建的模块。
// uri/parser/serializer/protocols/formats 模块由后续 Task 创建时再添加声明。
pub mod error;
pub mod formats;
pub mod model;
pub mod parser;
pub mod protocols;
pub mod serializer;
pub mod uri;

pub use error::{ParseError, SerializeError};
pub use model::{Crypto, GrpcConfig, HttpUpgradeConfig, Protocol, ProxyNode, ShadowTlsConfig, TlsSettings, Transport, WebsocketConfig};
