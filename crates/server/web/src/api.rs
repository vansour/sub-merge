// crates/server/web/src/api.rs
use gloo_net::http::{Method, Request, RequestBuilder};
use std::str::FromStr;
use submerge_web_core::error::ApiError;

/// 基础 fetch 封装。返回 body 字符串或错误。
pub async fn request(
    method: &str,
    path: &str,
    body: Option<String>,
    token: Option<&str>,
) -> Result<String, ApiError> {
    let mut builder = RequestBuilder::new(path);
    if let Ok(m) = Method::from_str(method) {
        builder = builder.method(m);
    }
    if let Some(t) = token {
        builder = builder.header("Authorization", &format!("Bearer {}", t));
    }
    // gloo-net 0.7: RequestBuilder::body(...) -> Result<Request, Error>
    // 有 body 时先构建 Request，无 body 时用 send() 直接发
    let resp = if let Some(b) = body {
        let req: Request = builder
            .header("Content-Type", "application/json")
            .body(b)
            .map_err(|e| ApiError {
                status: None,
                message: format!("body error: {e:?}"),
            })?;
        req.send().await.map_err(|e| ApiError {
            status: None,
            message: e.to_string(),
        })?
    } else {
        builder.send().await.map_err(|e| ApiError {
            status: None,
            message: e.to_string(),
        })?
    };
    let status = resp.status();
    let text = resp.text().await.map_err(|e| ApiError {
        status: None,
        message: e.to_string(),
    })?;
    if status >= 200 && status < 300 {
        Ok(text)
    } else {
        Err(ApiError {
            status: Some(status),
            message: format!("HTTP {status}: {text}"),
        })
    }
}
