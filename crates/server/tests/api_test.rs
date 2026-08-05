use server::config::AppConfig;
use server::db::init_db;
use sqlx::sqlite::SqlitePool;
use tower::ServiceExt;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn test_config(tmp: &std::path::Path) -> AppConfig {
    AppConfig {
        port: 0,
        db_path: tmp.join("test.db"),
        concurrency: 4,
        timeout_secs: 5,
        max_nodes: 100,
        web_dist: tmp.join("empty-dist"),
    }
}

async fn test_pool(tmp: &std::path::Path) -> SqlitePool {
    init_db(&tmp.join("test.db")).await.unwrap()
}

#[tokio::test]
async fn db_creates_tables_and_tokens() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-tokens", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;

    let (sub, admin) = server::db::ensure_tokens(&pool).await.unwrap();
    assert_eq!(sub.len(), 64); // 32 bytes hex
    assert_eq!(admin.len(), 64);
    assert_ne!(sub, admin);

    // 幂等：再次调用返回相同 token
    let (sub2, admin2) = server::db::ensure_tokens(&pool).await.unwrap();
    assert_eq!(sub, sub2);
    assert_eq!(admin, admin2);
}

#[tokio::test]
async fn unknown_route_returns_404() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-router", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let (_, admin) = server::db::ensure_tokens(&pool).await.unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg, admin).await;

    let resp = app
        .oneshot(Request::builder().uri("/api/nonexistent").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn subscribe_requires_token() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-sub-requires-token", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let (_sub, admin) = server::db::ensure_tokens(&pool).await.unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg, admin).await;

    // 无 token
    let resp = app.clone()
        .oneshot(Request::builder().uri("/api/subscribe").body(Body::empty()).unwrap())
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn subscribe_with_valid_token_returns_subscription() {
    let mock = MockServer::start().await;
    Mock::given(method("GET")).and(path("/sub"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#A\n"))
        .mount(&mock)
        .await;

    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-sub-valid", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let (sub, admin) = server::db::ensure_tokens(&pool).await.unwrap();

    // 插入一个指向 mock server 的源
    let url = format!("{}/sub", mock.uri());
    sqlx::query("INSERT INTO sources (url, name, enabled, created_at) VALUES (?, ?, 1, ?)")
        .bind(&url).bind("mock-source").bind("now").execute(&pool).await.unwrap();

    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg, admin).await;

    let resp = app.clone()
        .oneshot(Request::builder()
            .uri(format!("/api/subscribe?token={}&format=clash", sub))
            .body(Body::empty()).unwrap())
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(body.contains("proxies:"));
    assert!(body.contains("name: A"));
}

#[tokio::test]
async fn subscribe_wrong_format_returns_bad_request() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-sub-badfmt", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let (sub, admin) = server::db::ensure_tokens(&pool).await.unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg, admin).await;

    let resp = app.clone()
        .oneshot(Request::builder()
            .uri(format!("/api/subscribe?token={}&format=bogus", sub))
            .body(Body::empty()).unwrap())
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
