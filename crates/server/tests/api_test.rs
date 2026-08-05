use server::config::AppConfig;
use server::db::init_db;
use serde_json::json;
use sqlx::sqlite::SqlitePool;
use tower::ServiceExt;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

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

#[tokio::test]
async fn fetch_and_merge_respects_concurrency_cap() {
    // 最小 HTTP 源服务器：统计同一时刻的在途请求数，验证并发上限被遵守。
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));
    let served = Arc::new(AtomicUsize::new(0));
    {
        let active = active.clone();
        let max_active = max_active.clone();
        let served = served.clone();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            loop {
                if served.load(Ordering::SeqCst) >= 6 {
                    break;
                }
                let Ok((mut sock, _)) = listener.accept().await else { break };
                let active = active.clone();
                let max_active = max_active.clone();
                let served = served.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 2048];
                    let _ = sock.read(&mut buf).await;
                    let cur = active.fetch_add(1, Ordering::SeqCst) + 1;
                    max_active.fetch_max(cur, Ordering::SeqCst);
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    served.fetch_add(1, Ordering::SeqCst);
                    let body = b"ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#CONC\n";
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = sock.write_all(head.as_bytes()).await;
                    let _ = sock.write_all(body).await;
                    active.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
    }

    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-sub-concurrency", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let (_, admin) = server::db::ensure_tokens(&pool).await.unwrap();

    // 插入 6 个源，全部指向同一台并发计数服务器。
    for i in 0..6 {
        let url = format!("http://{}/s{}", addr, i);
        sqlx::query("INSERT INTO sources (url, name, enabled, created_at) VALUES (?, ?, 1, ?)")
            .bind(&url).bind(format!("src-{i}")).bind("now")
            .execute(&pool).await.unwrap();
    }

    let cfg = AppConfig {
        port: 0,
        db_path: tmp.join("test.db"),
        concurrency: 2,
        timeout_secs: 5,
        max_nodes: 100,
        web_dist: tmp.join("empty-dist"),
    };
    let state = server::state::AppState::new(pool, cfg, admin);

    let (nodes, errors) = server::service::fetch_and_merge(&state).await;

    assert!(errors.is_empty(), "expected no source errors, got {errors:?}");
    assert_eq!(nodes.len(), 6, "all 6 sources should be fetched and merged");
    assert!(nodes.iter().all(|n| n.name == "CONC"));
    assert_eq!(served.load(Ordering::SeqCst), 6, "server should have served all 6");
    let max = max_active.load(Ordering::SeqCst);
    assert!(max <= 2, "concurrency cap exceeded: {max} concurrent requests");
    assert!(max >= 2, "expected batching under the cap, got max concurrent {max}");
}

#[tokio::test]
async fn admin_requires_bearer_token() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-admin-noauth", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let (_, admin) = server::db::ensure_tokens(&pool).await.unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg, admin).await;

    // 无 header
    let resp = app.clone()
        .oneshot(Request::builder().uri("/api/admin/sources").body(Body::empty()).unwrap())
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 错误 token
    let resp = app.clone()
        .oneshot(Request::builder()
            .uri("/api/admin/sources")
            .header("authorization", "Bearer wrong")
            .body(Body::empty()).unwrap())
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn admin_crud_sources() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-admin-crud", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let (_, admin) = server::db::ensure_tokens(&pool).await.unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg, admin.clone()).await;

    let auth = |mut req: Request<Body>| {
        req.headers_mut().insert("authorization", format!("Bearer {}", admin).parse().unwrap());
        req
    };

    // create
    let resp = app.clone().oneshot(auth(Request::builder()
        .method("POST")
        .uri("/api/admin/sources")
        .header("content-type", "application/json")
        .body(Body::from(json!({"url": "https://example.com/sub", "name": "src1"}).to_string()))
        .unwrap()))
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let id = v["id"].as_i64().unwrap();

    // list
    let resp = app.clone().oneshot(auth(Request::builder().uri("/api/admin/sources").body(Body::empty()).unwrap())).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let list: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);

    // update enabled=false
    let resp = app.clone().oneshot(auth(Request::builder()
        .method("PUT")
        .uri(format!("/api/admin/sources/{}", id))
        .header("content-type", "application/json")
        .body(Body::from(json!({"enabled": false}).to_string()))
        .unwrap()))
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // delete
    let resp = app.clone().oneshot(auth(Request::builder()
        .method("DELETE")
        .uri(format!("/api/admin/sources/{}", id))
        .body(Body::empty()).unwrap()))
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // list empty
    let resp = app.clone().oneshot(auth(Request::builder().uri("/api/admin/sources").body(Body::empty()).unwrap())).await.unwrap();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let list: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 0);
}
