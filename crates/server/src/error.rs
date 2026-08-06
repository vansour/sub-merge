// crates/server/src/error.rs
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    pub fn unauthorized(msg: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: msg.into(),
        }
    }
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "bad_request",
            message: msg.into(),
        }
    }
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "not_found",
            message: msg.into(),
        }
    }
    pub fn internal(msg: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "internal_error",
            message: msg.into(),
        }
    }
    pub fn bad_gateway(msg: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_GATEWAY,
            code: "bad_gateway",
            message: msg.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(json!({ "error": { "code": self.code, "message": self.message } }));
        (self.status, body).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        tracing::error!("internal error: {e:?}");
        Self::internal(e.to_string())
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        tracing::error!("db error: {e:?}");
        Self::internal(e.to_string())
    }
}

// 提取器拒绝：让 Json/Path/Query 的默认 400/4xx 响应统一为 {error:{code,message}}。
// 保留各自实际 status（如 MissingJsonContentType 为 415），message 取自 axum 的拒绝描述。

impl From<JsonRejection> for ApiError {
    fn from(rej: JsonRejection) -> Self {
        Self {
            status: rej.status(),
            code: "invalid_json",
            message: rej.body_text(),
        }
    }
}

impl From<PathRejection> for ApiError {
    fn from(rej: PathRejection) -> Self {
        Self {
            status: rej.status(),
            code: "invalid_path",
            message: rej.body_text(),
        }
    }
}

impl From<QueryRejection> for ApiError {
    fn from(rej: QueryRejection) -> Self {
        Self {
            status: rej.status(),
            code: "invalid_query",
            message: rej.body_text(),
        }
    }
}
