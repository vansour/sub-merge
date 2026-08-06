// crates/server/web/src/api.rs
use gloo_net::http::{Method, Request, RequestBuilder};
use std::str::FromStr;

/// 请求错误：区分 HTTP 状态码与网络/解析错误。
/// status = Some(401) 表示鉴权失败（token 失效），调用方据此决定是否清除本地 token；
/// 网络错误/5xx 等瞬时错误 status 为 None 或非 401，不得清除 token。
#[derive(Debug)]
pub struct ApiError {
    pub status: Option<u16>,
    pub message: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl From<ApiError> for String {
    fn from(e: ApiError) -> String {
        e.message
    }
}

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
