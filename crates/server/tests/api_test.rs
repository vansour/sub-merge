use server::config::AppConfig;
use server::db::init_db;
use sqlx::sqlite::SqlitePool;
use tower::ServiceExt;
use axum::body::Body;
use axum::http::{Request, StatusCode};

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
