use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use server::config::AppConfig;
use server::db::init_db;
use sqlx::sqlite::SqlitePool;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tower::ServiceExt;
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

    let admin = server::db::ensure_tokens(&pool).await.unwrap();
    assert_eq!(admin.len(), 64); // 32 bytes hex

    // 幂等：再次调用返回相同 token
    let admin2 = server::db::ensure_tokens(&pool).await.unwrap();
    assert_eq!(admin, admin2);

    // 订阅 token 不再写入 settings
    let sub = server::db::get_setting(&pool, "subscribe_token")
        .await
        .unwrap();
    assert!(sub.is_none(), "subscribe_token must not be initialized");
}

#[tokio::test]
async fn env_preset_tokens_used_only_on_first_init() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-env-tokens", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;

    // 首次初始化：token 来源注入预设值（对应环境变量 SUB_MERGE_ADMIN_TOKEN 的行为）
    let admin = server::db::ensure_tokens_with(&pool, |key| {
        assert_eq!(key, "admin_token", "only admin_token is expected");
        Some("preset-admin-token-00000000000000000000000000000000".into())
    })
    .await
    .unwrap();
    assert_eq!(admin, "preset-admin-token-00000000000000000000000000000000");

    // 已有 token 时：即使注入新预设值也不覆盖（已部署实例 token 稳定）
    let admin2 = server::db::ensure_tokens_with(&pool, |_| {
        Some("another-preset-token-0000000000000000000000000000000".into())
    })
    .await
    .unwrap();
    assert_eq!(admin2, admin);

    // 无预设值则随机生成（原行为不变）：用全新 DB 验证
    let tmp2 =
        std::env::temp_dir().join(format!("submerge-test-{}-env-tokens2", std::process::id()));
    std::fs::create_dir_all(&tmp2).unwrap();
    let pool2 = test_pool(&tmp2).await;
    let admin3 = server::db::ensure_tokens_with(&pool2, |_| None)
        .await
        .unwrap();
    assert_eq!(admin3.len(), 64);
}

#[tokio::test]
async fn tokens_initialized_reflects_first_init() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-tok-init", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;

    // 全新 DB：未初始化
    assert!(!server::db::tokens_initialized(&pool).await.unwrap());

    // ensure_tokens 后：已初始化
    server::db::ensure_tokens(&pool).await.unwrap();
    assert!(server::db::tokens_initialized(&pool).await.unwrap());

    // 重启幂等：再次调用仍是已初始化（不触发首次打印）
    server::db::ensure_tokens(&pool).await.unwrap();
    assert!(server::db::tokens_initialized(&pool).await.unwrap());
}

#[tokio::test]
async fn unknown_route_returns_404() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-router", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let admin = server::db::ensure_tokens(&pool).await.unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg, admin).await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/nonexistent")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn subscribe_without_token_succeeds() {
    let tmp =
        std::env::temp_dir().join(format!("submerge-test-{}-sub-notoken", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let admin = server::db::ensure_tokens(&pool).await.unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg, admin).await;

    // 无任何 token 参数即可访问；无源时输出空 clash 配置
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/subscribe/merged?format=clash")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn subscribe_wrong_combined_name_returns_404() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-sub-404", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let admin = server::db::ensure_tokens(&pool).await.unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg, admin).await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/subscribe/not-a-sub")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.contains("application/json"),
        "expected JSON 404, got {ct:?}"
    );
}

#[tokio::test]
async fn subscribe_returns_subscription() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/sub"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string("ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#A\n"),
        )
        .mount(&mock)
        .await;

    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-sub-valid", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let admin = server::db::ensure_tokens(&pool).await.unwrap();

    // 插入一个指向 mock server 的源
    let url = format!("{}/sub", mock.uri());
    sqlx::query("INSERT INTO sources (url, name, enabled, created_at) VALUES (?, ?, 1, ?)")
        .bind(&url)
        .bind("mock-source")
        .bind("now")
        .execute(&pool)
        .await
        .unwrap();

    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg, admin).await;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/subscribe/merged?format=clash")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(body.contains("proxies:"));
    assert!(body.contains("name: A"));
}

#[tokio::test]
async fn subscribe_wrong_format_returns_bad_request() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-sub-badfmt", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let admin = server::db::ensure_tokens(&pool).await.unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg, admin).await;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/subscribe/merged?format=bogus")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
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
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
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

    let tmp = std::env::temp_dir().join(format!(
        "submerge-test-{}-sub-concurrency",
        std::process::id()
    ));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let admin = server::db::ensure_tokens(&pool).await.unwrap();

    // 插入 6 个源，全部指向同一台并发计数服务器。
    for i in 0..6 {
        let url = format!("http://{}/s{}", addr, i);
        sqlx::query("INSERT INTO sources (url, name, enabled, created_at) VALUES (?, ?, 1, ?)")
            .bind(&url)
            .bind(format!("src-{i}"))
            .bind("now")
            .execute(&pool)
            .await
            .unwrap();
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

    assert!(
        errors.is_empty(),
        "expected no source errors, got {errors:?}"
    );
    assert_eq!(nodes.len(), 6, "all 6 sources should be fetched and merged");
    assert!(nodes.iter().all(|n| n.name == "CONC"));
    assert_eq!(
        served.load(Ordering::SeqCst),
        6,
        "server should have served all 6"
    );
    let max = max_active.load(Ordering::SeqCst);
    assert!(
        max <= 2,
        "concurrency cap exceeded: {max} concurrent requests"
    );
    assert!(
        max >= 2,
        "expected batching under the cap, got max concurrent {max}"
    );
}

#[tokio::test]
async fn admin_requires_bearer_token() {
    let tmp =
        std::env::temp_dir().join(format!("submerge-test-{}-admin-noauth", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let admin = server::db::ensure_tokens(&pool).await.unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg, admin).await;

    // 无 header
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/sources")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 错误 token
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/sources")
                .header("authorization", "Bearer wrong")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn admin_crud_sources() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-admin-crud", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let admin = server::db::ensure_tokens(&pool).await.unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg, admin.clone()).await;

    let auth = |mut req: Request<Body>| {
        req.headers_mut().insert(
            "authorization",
            format!("Bearer {}", admin).parse().unwrap(),
        );
        req
    };

    // create
    let resp = app
        .clone()
        .oneshot(auth(
            Request::builder()
                .method("POST")
                .uri("/admin/sources")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"url": "https://example.com/sub", "name": "src1"}).to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let id = v["id"].as_i64().unwrap();

    // list
    let resp = app
        .clone()
        .oneshot(auth(
            Request::builder()
                .uri("/admin/sources")
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let list: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);

    // update enabled=false
    let resp = app
        .clone()
        .oneshot(auth(
            Request::builder()
                .method("PUT")
                .uri(format!("/admin/sources/{}", id))
                .header("content-type", "application/json")
                .body(Body::from(json!({"enabled": false}).to_string()))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // delete
    let resp = app
        .clone()
        .oneshot(auth(
            Request::builder()
                .method("DELETE")
                .uri(format!("/admin/sources/{}", id))
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // list empty
    let resp = app
        .clone()
        .oneshot(auth(
            Request::builder()
                .uri("/admin/sources")
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let list: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn preview_returns_node_list() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/sub"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string("ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#A\n"),
        )
        .mount(&mock)
        .await;

    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-preview", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let admin = server::db::ensure_tokens(&pool).await.unwrap();
    let url = format!("{}/sub", mock.uri());
    sqlx::query("INSERT INTO sources (url, name, enabled, created_at) VALUES (?, ?, 1, ?)")
        .bind(&url)
        .bind("mock")
        .bind("now")
        .execute(&pool)
        .await
        .unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg, admin.clone()).await;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/preview")
                .header("authorization", format!("Bearer {}", admin))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["total"], 1);
    assert_eq!(v["nodes"][0]["name"], "A");
}

#[tokio::test]
async fn config_get_and_rotate() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-config", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let admin = server::db::ensure_tokens(&pool).await.unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg, admin.clone()).await;

    let auth = |mut req: Request<Body>| {
        req.headers_mut().insert(
            "authorization",
            format!("Bearer {}", admin).parse().unwrap(),
        );
        req
    };

    // GET config：默认 merged、无订阅 token 字段
    let resp = app
        .clone()
        .oneshot(auth(
            Request::builder()
                .uri("/admin/config")
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["combined_name"], "merged");
    assert_eq!(v["subscribe_url"], "/subscribe/merged");
    assert!(
        v.get("subscribe_token").is_none(),
        "subscribe_token must be gone"
    );

    // 轮换 subscribe → 400（rotate 仅接受 admin）
    let resp = app
        .clone()
        .oneshot(auth(
            Request::builder()
                .method("PUT")
                .uri("/admin/config")
                .header("content-type", "application/json")
                .body(Body::from(json!({"rotate": "subscribe"}).to_string()))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 改组合订阅名 → 链接跟随
    let resp = app
        .clone()
        .oneshot(auth(
            Request::builder()
                .method("PUT")
                .uri("/admin/config")
                .header("content-type", "application/json")
                .body(Body::from(json!({"combined_name": "my-sub"}).to_string()))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["combined_name"], "my-sub");
    assert_eq!(v["subscribe_url"], "/subscribe/my-sub");

    // 非法名字 → 400
    let resp = app
        .clone()
        .oneshot(auth(
            Request::builder()
                .method("PUT")
                .uri("/admin/config")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"combined_name": "bad name!"}).to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn combined_name_rename_takes_effect() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-cfg-rename", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let admin = server::db::ensure_tokens(&pool).await.unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg, admin.clone()).await;

    let auth = |mut req: Request<Body>| {
        req.headers_mut().insert(
            "authorization",
            format!("Bearer {}", admin).parse().unwrap(),
        );
        req
    };
    // 改名 my-sub
    let resp = app
        .clone()
        .oneshot(auth(
            Request::builder()
                .method("PUT")
                .uri("/admin/config")
                .header("content-type", "application/json")
                .body(Body::from(json!({"combined_name": "my-sub"}).to_string()))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 新名字可访问，旧名字 404
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/subscribe/my-sub?format=clash")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/subscribe/merged?format=clash")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn admin_token_rotation_takes_effect_live() {
    let tmp =
        std::env::temp_dir().join(format!("submerge-test-{}-admin-rotate", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let admin = server::db::ensure_tokens(&pool).await.unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg, admin.clone()).await;

    let auth = |token: &str, req: Request<Body>| {
        let mut req = req;
        req.headers_mut().insert(
            "authorization",
            format!("Bearer {}", token).parse().unwrap(),
        );
        req
    };

    // 轮换 admin token（用旧 token 调 PUT）
    let resp = app
        .clone()
        .oneshot(auth(
            &admin,
            Request::builder()
                .method("PUT")
                .uri("/admin/config")
                .header("content-type", "application/json")
                .body(Body::from(json!({"rotate": "admin"}).to_string()))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let new_admin = v["admin_token"].as_str().unwrap().to_string();
    assert_ne!(new_admin, admin);

    // 旧 token 立即失效（内存锁已更新）
    let resp = app
        .clone()
        .oneshot(auth(
            &admin,
            Request::builder()
                .uri("/admin/config")
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 新 token 生效
    let resp = app
        .clone()
        .oneshot(auth(
            &new_admin,
            Request::builder()
                .uri("/admin/config")
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

async fn assert_error_json(resp: axum::response::Response, expected_code: &str) {
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "expected 400");
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes)
        .unwrap_or_else(|e| panic!("response body is not JSON: {e:?} -> {:?}", bytes));
    assert_eq!(
        v["error"]["code"], expected_code,
        "unexpected error code: {v}"
    );
    assert!(
        v["error"]["message"].is_string(),
        "missing error.message: {v}"
    );
    assert!(!v["error"]["message"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn rejection_non_numeric_id_returns_unified_json() {
    // 回归：PUT /admin/sources/abc 的非数字 {id} 应走统一错误格式
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-rej-path", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let admin = server::db::ensure_tokens(&pool).await.unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg, admin.clone()).await;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/admin/sources/abc")
                .header("authorization", format!("Bearer {}", admin))
                .header("content-type", "application/json")
                .body(Body::from(json!({"enabled": false}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_error_json(resp, "invalid_path").await;
}

#[tokio::test]
async fn rejection_malformed_json_returns_unified_json() {
    // 回归：POST /admin/sources 的 malformed JSON body 应走统一错误格式
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-rej-json", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let admin = server::db::ensure_tokens(&pool).await.unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg, admin.clone()).await;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/sources")
                .header("authorization", format!("Bearer {}", admin))
                .header("content-type", "application/json")
                .body(Body::from("{\"url\": \"\"")) // 语法错误的 JSON
                .unwrap(),
        )
        .await
        .unwrap();
    assert_error_json(resp, "invalid_json").await;
}

#[tokio::test]
async fn subscribe_skips_unserializable_node_instead_of_500() {
    // 源包含一个可解析但无法序列化的 wireguard 节点（缺 privateKey）+ 一个正常 ss 节点
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/sub"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#OK\n\
             wireguard://cHVibGljS2V5MTIz@1.2.3.4:443?publicKey=cHVibGljS2V5MTIz#WG\n",
        ))
        .mount(&mock)
        .await;

    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-wg-skip", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let admin = server::db::ensure_tokens(&pool).await.unwrap();
    let url = format!("{}/sub", mock.uri());
    sqlx::query("INSERT INTO sources (url, name, enabled, created_at) VALUES (?, ?, 1, ?)")
        .bind(&url)
        .bind("mock")
        .bind("now")
        .execute(&pool)
        .await
        .unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg, admin).await;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/subscribe/merged?format=clash")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "bad node must not 500 the subscription"
    );
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(body.contains("name: OK"), "good node must survive");
    assert!(!body.contains("WG"), "unserializable node must be skipped");
}

#[tokio::test]
async fn api_path_variants_return_json_404() {
    let tmp =
        std::env::temp_dir().join(format!("submerge-test-{}-api-variants", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    // 构造含 index.html 的 dist，确保 SPA 回退存在（若被绕过会返回 HTML 200）
    let dist = tmp.join("web-dist");
    std::fs::create_dir_all(&dist).unwrap();
    std::fs::write(dist.join("index.html"), "<html>sub-merge</html>").unwrap();

    let pool = test_pool(&tmp).await;
    let admin = server::db::ensure_tokens(&pool).await.unwrap();
    let cfg = AppConfig {
        web_dist: dist,
        ..test_config(&tmp)
    };
    let app = server::routes::build_router(pool, cfg, admin).await;

    for path in [
        "/api",
        "//admin/sources",
        "/api%2Fadmin/preview",
        "/admin",
        "//api/admin/sources",
        "/admin%2Fsources/preview",
        "/subscribe",
        "//subscribe/whatever",
    ] {
        let resp = app
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "path {path} must be JSON 404"
        );
        let ct = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(
            ct.contains("application/json"),
            "path {path} must return JSON, got {ct:?}"
        );
    }
}

#[tokio::test]
async fn static_index_served_from_dist() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-static", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    // 创建一个假 web-dist：index.html + 一个静态资源
    let dist = tmp.join("web-dist");
    std::fs::create_dir_all(&dist).unwrap();
    std::fs::write(dist.join("index.html"), "<html>sub-merge</html>").unwrap();
    std::fs::write(dist.join("app.css"), "body{color:red}").unwrap();

    let pool = test_pool(&tmp).await;
    let admin = server::db::ensure_tokens(&pool).await.unwrap();
    let cfg = AppConfig {
        port: 0,
        db_path: tmp.join("test.db"),
        concurrency: 4,
        timeout_secs: 5,
        max_nodes: 100,
        web_dist: dist.clone(),
    };
    let app = server::routes::build_router(pool, cfg.clone(), admin.clone()).await;

    // 健康检查走 /healthz（不经过 fallback）
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&bytes[..], b"sub-merge is running");

    // 根路径返回 SPA index.html（fallback 接管，浏览器直接打开 / 即见管理界面）
    let resp = app
        .clone()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&bytes[..], b"<html>sub-merge</html>");

    // 静态资源从 dist 目录提供，带正确的 content-type
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/app.css")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&bytes[..], b"body{color:red}");
    assert!(ct.starts_with("text/css"), "unexpected content-type: {ct}");

    // SPA 回退：不存在的路径返回 index.html
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/some/spa/route")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&bytes[..], b"<html>sub-merge</html>");

    // 路径穿越 → 403
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/../etc/passwd")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // dist 目录不存在时 fallback 返回 404
    let empty_cfg = AppConfig {
        web_dist: tmp.join("no-such-dist"),
        ..cfg
    };
    let app = server::routes::build_router(test_pool(&tmp).await, empty_cfg, admin).await;
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/missing.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn legacy_db_without_kind_column_is_migrated() {
    // 模拟早期版本建的表（无 kind 列）：init_db 应 ALTER 迁移成功并保留数据
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-legacy", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let db_path = tmp.join("test.db");
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_lazy_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&db_path)
                .create_if_missing(true),
        );
    sqlx::query(
        "CREATE TABLE sources (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            url TEXT NOT NULL,
            name TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO sources (url, name, enabled, created_at) VALUES ('https://old.example/sub', 'old', 1, '2026-01-01')",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;

    // init_db 迁移后：kind 列存在、旧源默认为 remote
    let pool = init_db(&db_path).await.unwrap();
    let kind: String = sqlx::query_scalar("SELECT kind FROM sources WHERE name = 'old'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(kind, "remote");
}

#[tokio::test]
async fn single_source_parses_without_network() {
    // single 源指向一个无法连通的地址（127.0.0.1:1）；若代码误发请求必然失败进错误列表，
    // 正确实现（直接解析）则节点正常出现。
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-single", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let admin = server::db::ensure_tokens(&pool).await.unwrap();
    let cfg = test_config(&tmp);
    let state = server::state::AppState::new(pool, cfg, admin);

    // remote 源：正常 mock
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/sub"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#REMOTE\n"),
        )
        .mount(&mock)
        .await;
    let url = format!("{}/sub", mock.uri());
    sqlx::query(
        "INSERT INTO sources (url, name, kind, enabled, created_at) VALUES (?, ?, ?, 1, ?)",
    )
    .bind(&url)
    .bind("remote-src")
    .bind("remote")
    .bind("now")
    .execute(&state.pool)
    .await
    .unwrap();
    // single 源：服务器地址 127.0.0.1:1（连接必然失败），若被 fetch 则产生源错误
    sqlx::query(
        "INSERT INTO sources (url, name, kind, enabled, created_at) VALUES (?, ?, ?, 1, ?)",
    )
    .bind("ss://YWVzLTI1Ni1nY206cGFzcw@127.0.0.1:1#SINGLE")
    .bind("single-src")
    .bind("single")
    .bind("now")
    .execute(&state.pool)
    .await
    .unwrap();

    let (nodes, errors) = server::service::fetch_and_merge(&state).await;
    assert!(
        errors.is_empty(),
        "expected no source errors, got {errors:?}"
    );
    assert_eq!(nodes.len(), 2);
    assert!(
        nodes.iter().any(|n| n.name == "SINGLE"),
        "single node must be parsed"
    );
    assert!(nodes.iter().any(|n| n.name == "REMOTE"));
}

#[tokio::test]
async fn invalid_single_source_reports_source_error() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-single-bad", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let admin = server::db::ensure_tokens(&pool).await.unwrap();
    let cfg = test_config(&tmp);
    let state = server::state::AppState::new(pool, cfg, admin);

    sqlx::query(
        "INSERT INTO sources (url, name, kind, enabled, created_at) VALUES (?, ?, ?, 1, ?)",
    )
    .bind("this is not a node uri")
    .bind("bad-src")
    .bind("single")
    .bind("now")
    .execute(&state.pool)
    .await
    .unwrap();

    let (nodes, errors) = server::service::fetch_and_merge(&state).await;
    assert!(nodes.is_empty());
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].source_name, "bad-src");
    assert!(
        errors[0].reason.contains("parse failed"),
        "reason: {}",
        errors[0].reason
    );
}

#[tokio::test]
async fn admin_crud_respects_kind() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-kind", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let admin = server::db::ensure_tokens(&pool).await.unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg, admin.clone()).await;

    let auth = |mut req: Request<Body>| {
        req.headers_mut().insert(
            "authorization",
            format!("Bearer {}", admin).parse().unwrap(),
        );
        req
    };

    // 创建 kind=single 源 → 创建响应带 kind
    let resp = app
        .clone()
        .oneshot(auth(
            Request::builder()
                .method("POST")
                .uri("/admin/sources")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"url": "ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#S", "name": "s1", "kind": "single"})
                        .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["kind"], "single");
    let id = v["id"].as_i64().unwrap();

    // 不传 kind → 默认 remote
    let resp = app
        .clone()
        .oneshot(auth(
            Request::builder()
                .method("POST")
                .uri("/admin/sources")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"url": "https://x/sub", "name": "s2"}).to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["kind"], "remote");

    // 非法 kind → 400
    let resp = app
        .clone()
        .oneshot(auth(
            Request::builder()
                .method("POST")
                .uri("/admin/sources")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"url": "https://x/sub", "name": "s3", "kind": "bogus"}).to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // PUT 改 kind
    let resp = app
        .clone()
        .oneshot(auth(
            Request::builder()
                .method("PUT")
                .uri(format!("/admin/sources/{}", id))
                .header("content-type", "application/json")
                .body(Body::from(json!({"kind": "remote"}).to_string()))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["kind"], "remote");
}

#[tokio::test]
async fn refresh_single_source_reports_locally() {
    let tmp = std::env::temp_dir().join(format!(
        "submerge-test-{}-single-refresh",
        std::process::id()
    ));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let admin = server::db::ensure_tokens(&pool).await.unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool.clone(), cfg, admin.clone()).await;

    let auth = |mut req: Request<Body>| {
        req.headers_mut().insert(
            "authorization",
            format!("Bearer {}", admin).parse().unwrap(),
        );
        req
    };

    // kind=single 源：本地解析单条 ss 链接，不拉网络
    let res = sqlx::query(
        "INSERT INTO sources (url, name, kind, enabled, created_at) VALUES (?, ?, 'single', 1, ?)",
    )
    .bind("ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#S")
    .bind("single-ok")
    .bind("now")
    .execute(&pool)
    .await
    .unwrap();
    let id = res.last_insert_rowid();

    let resp = app
        .clone()
        .oneshot(auth(
            Request::builder()
                .method("POST")
                .uri(format!("/admin/sources/{id}/refresh"))
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["ok"], serde_json::Value::Bool(true));
    assert_eq!(v["node_count"], serde_json::Value::Number(1.into()));

    // kind=single 源：URI 非法 → ok false，reason 含 parse failed
    let res = sqlx::query(
        "INSERT INTO sources (url, name, kind, enabled, created_at) VALUES (?, ?, 'single', 1, ?)",
    )
    .bind("not a uri")
    .bind("single-bad")
    .bind("now")
    .execute(&pool)
    .await
    .unwrap();
    let id2 = res.last_insert_rowid();

    let resp = app
        .clone()
        .oneshot(auth(
            Request::builder()
                .method("POST")
                .uri(format!("/admin/sources/{id2}/refresh"))
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["ok"], serde_json::Value::Bool(false));
    assert!(
        v["reason"].as_str().unwrap().contains("parse failed"),
        "reason must mention parse failure, got {:?}",
        v["reason"]
    );
}
