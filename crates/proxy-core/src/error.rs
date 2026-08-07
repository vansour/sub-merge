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
    #[error("invalid clash template: {0}")]
    InvalidTemplate(String),
}
