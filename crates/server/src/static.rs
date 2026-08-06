// crates/server/src/static.rs
use crate::error::ApiError;
use crate::state::AppState;
use axum::body::Body;
use axum::extract::State;
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};

/// 从 web_dist 目录提供静态资源。SPA 回退：找不到文件时返回 index.html。
pub async fn fallback(State(state): State<AppState>, uri: Uri) -> Response {
    // 未知的 API 命名空间路径（/api/*、/admin/*、/subscribe/*，含 // 双斜杠与编码斜杠形态）
    // 绝不回退到 SPA index.html，返回统一 JSON 404。其余路径（含前端路由）回退 SPA。
    let p = uri.path().trim_start_matches('/');
    if p.starts_with("api") || p.starts_with("admin") || p.starts_with("subscribe") {
        return ApiError::not_found("route not found").into_response();
    }

    let root = state.cfg.web_dist.clone();
    let rel_path = uri.path().trim_start_matches('/');
    let rel_path = if rel_path.is_empty() {
        "index.html"
    } else {
        rel_path
    };

    // 防止路径穿越
    if rel_path.contains("..") {
        return StatusCode::FORBIDDEN.into_response();
    }

    let candidate = root.join(rel_path);
    let mime = mime_guess::from_path(&candidate).first_or_octet_stream();

    match tokio::fs::read(&candidate).await {
        Ok(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime.as_ref())
            .body(Body::from(bytes))
            .unwrap(),
        Err(_) => {
            // SPA fallback: 返回 index.html（若存在）
            let index = root.join("index.html");
            match tokio::fs::read(&index).await {
                Ok(bytes) => Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "text/html")
                    .body(Body::from(bytes))
                    .unwrap(),
                Err(_) => StatusCode::NOT_FOUND.into_response(),
            }
        }
    }
}
