// Regression tests for final-review fixes:
// 1. Unknown /api/* paths must return a unified JSON 404, never SPA-fallback to index.html.
// 2. refresh_source reports ok:false when the source body parses to zero nodes.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use server::config::AppConfig;
use server::db::init_db;
use server::routes::build_router;
use tower::ServiceExt;

fn unique_dir(tag: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("submerge-{tag}-{}-{nanos}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir); // 清理同名残留（nanos 唯一性之外的兜底）
    dir
}

#[tokio::test]
async fn api_unknown_path_returns_json_404_not_spa() {
    // web_dist 中存在 index.html 时，未知 /api 路径也必须返回 JSON 404，而不是 SPA 回退的 HTML。
    let dir = unique_dir("api404");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), "<!DOCTYPE html><title>SPA</title>").unwrap();

    let db_path = dir.join("test.db");
    let pool = init_db(&db_path).await.unwrap();
    let cfg = AppConfig {
        port: 0,
        db_path: db_path.clone(),
        concurrency: 4,
        timeout_secs: 5,
        max_nodes: 100,
        web_dist: dir.clone(),
        session_ttl_days: 0,
    };
    let app = build_router(pool, cfg).await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/admin/does-not-exist")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let ct = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.contains("application/json"),
        "expected application/json, got {ct:?}"
    );

    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["error"]["code"], "not_found");
    assert_eq!(body["error"]["message"], "route not found");

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn refresh_source_zero_nodes_reports_ok_false() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/empty"))
        .respond_with(ResponseTemplate::new(200).set_body_string("this is not a subscription body"))
        .mount(&mock)
        .await;

    let dir = unique_dir("refresh-zero");
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("test.db");
    let pool = init_db(&db_path).await.unwrap();
    let cfg = AppConfig {
        port: 0,
        db_path: db_path.clone(),
        concurrency: 4,
        timeout_secs: 10,
        max_nodes: 100,
        web_dist: dir.clone(),
        session_ttl_days: 0,
    };
    let app = build_router(pool.clone(), cfg).await;
    // 会话鉴权：直接走 db 层创建用户与会话
    server::db::create_user(&pool, "admin", "pass-12345")
        .await
        .unwrap();
    let session = server::db::create_session(&pool).await.unwrap();

    // 指向返回空/零节点内容的 URL。
    let url = format!("{}/empty", mock.uri());
    sqlx::query("INSERT INTO sources (url, name, enabled, created_at) VALUES (?, ?, 1, ?)")
        .bind(&url)
        .bind("empty-src")
        .bind("2026-01-01T00:00:00Z")
        .execute(&pool)
        .await
        .unwrap();

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/sources/1/refresh")
                .header("authorization", format!("Bearer {session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["ok"], serde_json::Value::Bool(false));
    assert_eq!(body["reason"], "no nodes parsed");
    assert_eq!(body["node_count"], serde_json::Value::Null);

    let _ = std::fs::remove_dir_all(&dir);
}
