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

fn fresh_tmp(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("submerge-test-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir); // 清理 PID 复用残留
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn test_config(tmp: &std::path::Path) -> AppConfig {
    AppConfig {
        port: 0,
        db_path: tmp.join("test.db"),
        concurrency: 4,
        timeout_secs: 5,
        max_nodes: 100,
        web_dist: tmp.join("empty-dist"),
        session_ttl_days: 0, // 测试默认禁用过期：既有测试语义不受 TTL 影响
    }
}

async fn test_pool(tmp: &std::path::Path) -> SqlitePool {
    init_db(&tmp.join("test.db")).await.unwrap()
}

fn valid_setup(name: &str) -> String {
    json!({"username": name, "password": "pass-12345", "password_confirm": "pass-12345"})
        .to_string()
}

async fn http(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<String>,
    token: Option<&str>,
) -> (axum::http::StatusCode, serde_json::Value) {
    let mut b = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "example.com"); // 统一默认 Host：clash 订阅分支依赖 Host 拼 provider url
    if let Some(t) = token {
        b = b.header("authorization", format!("Bearer {t}"));
    }
    let req = match body {
        Some(s) => b
            .header("content-type", "application/json")
            .body(axum::body::Body::from(s))
            .unwrap(),
        None => b.body(axum::body::Body::empty()).unwrap(),
    };
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, v)
}

/// 同 `http`，但返回原始 body 字符串（非 JSON Value），固定 Host 供 provider url 断言
async fn http_raw(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<String>,
    token: Option<&str>,
) -> (axum::http::StatusCode, String) {
    let mut b = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "example.com"); // 固定 Host 供 provider url 断言
    if let Some(t) = token {
        b = b.header("authorization", format!("Bearer {t}"));
    }
    let req = match body {
        Some(s) => b
            .header("content-type", "application/json")
            .body(axum::body::Body::from(s))
            .unwrap(),
        None => b.body(axum::body::Body::empty()).unwrap(),
    };
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

/// 测试内联 sha256（hex）：不回退导出 db.rs 内部函数
fn sha256_hex_manual(s: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    const_hex::encode(h.finalize())
}

/// v2ray 订阅输出是 base64，测试断言前解码
fn b64_decode(s: &str) -> String {
    use base64::Engine;
    String::from_utf8(
        base64::engine::general_purpose::STANDARD
            .decode(s.trim())
            .unwrap_or_default(),
    )
    .unwrap_or_default()
}

/// 走真实 HTTP 链路创建管理员并登录，返回会话 token
async fn setup_admin(app: &axum::Router) -> String {
    let (s, _) = http(
        app,
        "POST",
        "/admin/setup",
        Some(valid_setup("admin")),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "setup must succeed");
    let (s, v) = http(
        app,
        "POST",
        "/admin/login",
        Some(json!({"username": "admin", "password": "pass-12345"}).to_string()),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "login must succeed");
    v["token"].as_str().unwrap().to_string()
}

#[test]
fn parse_bearer_three_states() {
    use axum::http::HeaderMap;
    use axum::http::header::{AUTHORIZATION, HeaderValue};

    let no_header = HeaderMap::new();
    assert_eq!(
        server::auth::parse_bearer(&no_header),
        Err("missing authorization header")
    );

    let mut wrong_scheme = HeaderMap::new();
    wrong_scheme.insert(AUTHORIZATION, HeaderValue::from_static("Basic abc"));
    assert_eq!(
        server::auth::parse_bearer(&wrong_scheme),
        Err("expected Bearer token")
    );

    let mut ok = HeaderMap::new();
    ok.insert(AUTHORIZATION, HeaderValue::from_static("Bearer tok123"));
    assert_eq!(server::auth::parse_bearer(&ok), Ok("tok123"));
}

#[tokio::test]
async fn unknown_route_returns_404() {
    let tmp = fresh_tmp("router");
    let pool = test_pool(&tmp).await;
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg).await;

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
    let tmp = fresh_tmp("sub-notoken");
    let pool = test_pool(&tmp).await;
    // 空成员组合：clash 分支不拉源，输出默认模板订阅组配置
    sqlx::query("INSERT INTO combined_subs (name, created_at) VALUES ('merged', 'now')")
        .execute(&pool)
        .await
        .unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg).await;

    // 无任何 token 参数即可访问；clash 分支需要 Host header 拼 provider url
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/subscribe/merged?format=clash")
                .header("host", "example.com")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn subscribe_wrong_combined_name_returns_404() {
    let tmp = fresh_tmp("sub-404");
    let pool = test_pool(&tmp).await;
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg).await;

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

    let tmp = fresh_tmp("sub-valid");
    let pool = test_pool(&tmp).await;

    // 插入一个指向 mock server 的源
    let url = format!("{}/sub", mock.uri());
    let res =
        sqlx::query("INSERT INTO sources (url, name, enabled, created_at) VALUES (?, ?, 1, ?)")
            .bind(&url)
            .bind("mock-source")
            .bind("now")
            .execute(&pool)
            .await
            .unwrap();
    let src_id = res.last_insert_rowid();
    // 建组合勾选该源
    sqlx::query("INSERT INTO combined_subs (name, created_at) VALUES ('merged', 'now')")
        .execute(&pool)
        .await
        .unwrap();
    let cid: i64 = sqlx::query_scalar("SELECT id FROM combined_subs WHERE name = 'merged'")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO combined_sources (combined_id, source_id) VALUES (?, ?)")
        .bind(cid)
        .bind(src_id)
        .execute(&pool)
        .await
        .unwrap();

    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg).await;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/subscribe/merged?format=clash")
                .header("host", "example.com")
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
    // clash 输出已改为订阅组模式：模板 + providers 引用（不再输出解析节点）
    assert!(body.contains("proxy-providers:"), "订阅组模式输出");
    assert!(body.contains("merged:"), "provider key = 组合名");
    assert!(
        body.contains("url: http://example.com/subscribe/merged?format=v2ray"),
        "provider url 拼请求 Host"
    );
    assert!(body.contains("proxies:"), "模板代理组段保留");
}

#[tokio::test]
async fn subscribe_wrong_format_returns_bad_request() {
    let tmp = fresh_tmp("sub-badfmt");
    let pool = test_pool(&tmp).await;
    // 组合必须存在，format 校验才会被触达
    sqlx::query("INSERT INTO combined_subs (name, created_at) VALUES ('merged', 'now')")
        .execute(&pool)
        .await
        .unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg).await;

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

    let tmp = fresh_tmp("sub-concurrency");
    let pool = test_pool(&tmp).await;

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
        session_ttl_days: 0,
    };
    let state = server::state::AppState::new(pool, cfg);

    let (nodes, errors) = server::service::fetch_and_merge(&state, None).await;

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
    let tmp = fresh_tmp("admin-noauth");
    let pool = test_pool(&tmp).await;
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg).await;

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
    let tmp = fresh_tmp("admin-crud");
    let pool = test_pool(&tmp).await;
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg).await;
    let admin = setup_admin(&app).await;

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

    let tmp = fresh_tmp("preview");
    let pool = test_pool(&tmp).await;
    let url = format!("{}/sub", mock.uri());
    sqlx::query("INSERT INTO sources (url, name, enabled, created_at) VALUES (?, ?, 1, ?)")
        .bind(&url)
        .bind("mock")
        .bind("now")
        .execute(&pool)
        .await
        .unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg).await;
    let admin = setup_admin(&app).await;

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
async fn config_returns_username() {
    let tmp = fresh_tmp("cfg-user");
    let pool = test_pool(&tmp).await;
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg).await;
    let admin = setup_admin(&app).await;

    let (s, v) = http(&app, "GET", "/admin/config", None, Some(&admin)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["username"], "admin");
    assert!(v.get("admin_token").is_none(), "admin_token must be gone");
}

#[tokio::test]
async fn change_password_invalidates_all_sessions() {
    let tmp = fresh_tmp("chpass");
    let pool = test_pool(&tmp).await;
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg).await;
    let admin = setup_admin(&app).await;
    // 第二台设备：再登录拿一个会话
    let (_, v) = http(
        &app,
        "POST",
        "/admin/login",
        Some(json!({"username": "admin", "password": "pass-12345"}).to_string()),
        None,
    )
    .await;
    let second = v["token"].as_str().unwrap().to_string();

    // 旧密码错误 → 400
    let (s, _) = http(
        &app,
        "PUT",
        "/admin/config",
        Some(json!({"change_password": {"old": "wrong", "new": "new-pass-678"}}).to_string()),
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // 新密码过短 → 400
    let (s, _) = http(
        &app,
        "PUT",
        "/admin/config",
        Some(json!({"change_password": {"old": "pass-12345", "new": "short"}}).to_string()),
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // 正确改密 → 200，返回 username
    let (s, v) = http(
        &app,
        "PUT",
        "/admin/config",
        Some(json!({"change_password": {"old": "pass-12345", "new": "new-pass-678"}}).to_string()),
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["username"], "admin");

    // 全部旧会话（含当前）立即失效 → 401
    let (s, _) = http(&app, "GET", "/admin/config", None, Some(&admin)).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = http(&app, "GET", "/admin/config", None, Some(&second)).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // 新密码可登录
    let (s, v) = http(
        &app,
        "POST",
        "/admin/login",
        Some(json!({"username": "admin", "password": "new-pass-678"}).to_string()),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let new_token = v["token"].as_str().unwrap().to_string();
    let (s, _) = http(&app, "GET", "/admin/config", None, Some(&new_token)).await;
    assert_eq!(s, StatusCode::OK);
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
    let tmp = fresh_tmp("rej-path");
    let pool = test_pool(&tmp).await;
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg).await;
    let admin = setup_admin(&app).await;

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
    let tmp = fresh_tmp("rej-json");
    let pool = test_pool(&tmp).await;
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg).await;
    let admin = setup_admin(&app).await;

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
async fn subscribe_skips_unsupported_protocol_in_v2ray() {
    // 源含一个 wireguard 节点 + 一个正常 ss 节点。v2ray 序列化对 wireguard 是
    // 显式协议排除（serialize_v2ray 的 continue），不是错误容错——本用例只验证
    // 该协议排除行为；「可解析但序列化失败节点被跳过」的错误容错由 singbox 用例覆盖。
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/sub"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#OK\n\
             wireguard://cHVibGljS2V5MTIz@1.2.3.4:443?publicKey=cHVibGljS2V5MTIz#WG\n",
        ))
        .mount(&mock)
        .await;

    let tmp = fresh_tmp("wg-skip");
    let pool = test_pool(&tmp).await;
    let url = format!("{}/sub", mock.uri());
    let res =
        sqlx::query("INSERT INTO sources (url, name, enabled, created_at) VALUES (?, ?, 1, ?)")
            .bind(&url)
            .bind("mock")
            .bind("now")
            .execute(&pool)
            .await
            .unwrap();
    let src_id = res.last_insert_rowid();
    // 建组合勾选该源
    sqlx::query("INSERT INTO combined_subs (name, created_at) VALUES ('merged', 'now')")
        .execute(&pool)
        .await
        .unwrap();
    let cid: i64 = sqlx::query_scalar("SELECT id FROM combined_subs WHERE name = 'merged'")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO combined_sources (combined_id, source_id) VALUES (?, ?)")
        .bind(cid)
        .bind(src_id)
        .execute(&pool)
        .await
        .unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg).await;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/subscribe/merged?format=v2ray")
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
    let decoded = b64_decode(&body);
    assert!(decoded.contains("#OK"), "good node must survive");
    assert!(
        !decoded.contains("WG"),
        "wireguard must be excluded from v2ray output"
    );
}

#[tokio::test]
async fn subscribe_singbox_skips_unserializable_node_instead_of_500() {
    // 错误容错回归：源含一个「可解析但 singbox 序列化失败」的 wireguard 节点
    // （缺 privateKey → node_to_singbox 返回 Err，filter_map 跳过）+ 一个正常 ss 节点。
    // 订阅必须 200，坏节点被跳过而非拖垮整个输出。
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/sub"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#OK\n\
             wireguard://cHVibGljS2V5MTIz@1.2.3.4:443?publicKey=cHVibGljS2V5MTIz#WG\n",
        ))
        .mount(&mock)
        .await;

    let tmp = fresh_tmp("singbox-wg-skip");
    let pool = test_pool(&tmp).await;
    let url = format!("{}/sub", mock.uri());
    let res =
        sqlx::query("INSERT INTO sources (url, name, enabled, created_at) VALUES (?, ?, 1, ?)")
            .bind(&url)
            .bind("mock")
            .bind("now")
            .execute(&pool)
            .await
            .unwrap();
    let src_id = res.last_insert_rowid();
    sqlx::query("INSERT INTO combined_subs (name, created_at) VALUES ('merged', 'now')")
        .execute(&pool)
        .await
        .unwrap();
    let cid: i64 = sqlx::query_scalar("SELECT id FROM combined_subs WHERE name = 'merged'")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO combined_sources (combined_id, source_id) VALUES (?, ?)")
        .bind(cid)
        .bind(src_id)
        .execute(&pool)
        .await
        .unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg).await;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/subscribe/merged?format=singbox")
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
    assert!(body.contains("\"OK\""), "good node must survive: {body}");
    assert!(
        !body.contains("WG"),
        "unserializable node must be skipped: {body}"
    );
}

#[tokio::test]
async fn api_path_variants_return_json_404() {
    let tmp = fresh_tmp("api-variants");
    // 构造含 index.html 的 dist，确保 SPA 回退存在（若被绕过会返回 HTML 200）
    let dist = tmp.join("web-dist");
    std::fs::create_dir_all(&dist).unwrap();
    std::fs::write(dist.join("index.html"), "<html>sub-merge</html>").unwrap();

    let pool = test_pool(&tmp).await;
    let cfg = AppConfig {
        web_dist: dist,
        ..test_config(&tmp)
    };
    let app = server::routes::build_router(pool, cfg).await;

    for path in [
        "/api",
        "//admin/sources",
        "/api%2Fadmin/preview",
        "/admin",
        "//api/admin/sources",
        "/admin%2Fsources/preview",
        "/subscribe",
        "//subscribe/whatever",
        // 百分号编码字母形态：解码后才能命中前缀守卫
        "/sub%73cribe/x",
        "/adm%69n/config",
        "/%61pi/admin/x",
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
    let tmp = fresh_tmp("static");
    // 创建一个假 web-dist：index.html + 一个静态资源
    let dist = tmp.join("web-dist");
    std::fs::create_dir_all(&dist).unwrap();
    std::fs::write(dist.join("index.html"), "<html>sub-merge</html>").unwrap();
    std::fs::write(dist.join("app.css"), "body{color:red}").unwrap();

    let pool = test_pool(&tmp).await;
    let cfg = AppConfig {
        port: 0,
        db_path: tmp.join("test.db"),
        concurrency: 4,
        timeout_secs: 5,
        max_nodes: 100,
        web_dist: dist.clone(),
        session_ttl_days: 0,
    };
    let app = server::routes::build_router(pool, cfg.clone()).await;

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
    let app = server::routes::build_router(test_pool(&tmp).await, empty_cfg).await;
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
    let tmp = fresh_tmp("legacy");
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
async fn legacy_db_without_last_used_at_column_is_migrated() {
    // 模拟认证改造前的旧库（sessions 无 last_used_at 列）：
    // init_db 应 ALTER 迁移成功、默认值填充 last_used_at，旧会话迁移后仍有效。
    let tmp = fresh_tmp("legacy-session");
    let db_path = tmp.join("test.db");
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_lazy_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&db_path)
                .create_if_missing(true),
        );
    sqlx::query(
        "CREATE TABLE sessions (
            token_hash TEXT PRIMARY KEY,
            created_at TEXT NOT NULL
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO sessions (token_hash, created_at) VALUES (?, '2026-01-01T00:00:00Z')")
        .bind(sha256_hex_manual("legacy-token"))
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    // init_db 迁移后：旧会话获得默认 last_used_at（DEFAULT '' + strftime 回填为 RFC3339），TTL=30 下仍有效
    let pool = init_db(&db_path).await.unwrap();
    assert!(
        server::db::validate_session(&pool, "legacy-token", 30)
            .await
            .unwrap(),
        "migrated legacy session must validate"
    );
}

#[tokio::test]
async fn single_source_parses_without_network() {
    // single 源指向一个无法连通的地址（127.0.0.1:1）；若代码误发请求必然失败进错误列表，
    // 正确实现（直接解析）则节点正常出现。
    let tmp = fresh_tmp("single");
    let pool = test_pool(&tmp).await;
    let cfg = test_config(&tmp);
    let state = server::state::AppState::new(pool, cfg);

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

    let (nodes, errors) = server::service::fetch_and_merge(&state, None).await;
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
    let tmp = fresh_tmp("single-bad");
    let pool = test_pool(&tmp).await;
    let cfg = test_config(&tmp);
    let state = server::state::AppState::new(pool, cfg);

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

    let (nodes, errors) = server::service::fetch_and_merge(&state, None).await;
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
    let tmp = fresh_tmp("kind");
    let pool = test_pool(&tmp).await;
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg).await;
    let admin = setup_admin(&app).await;

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
    let tmp = fresh_tmp("single-refresh");
    let pool = test_pool(&tmp).await;
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool.clone(), cfg).await;
    let admin = setup_admin(&app).await;

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

#[tokio::test]
async fn combined_tables_and_cascade() {
    let tmp = fresh_tmp("combined-tbl");
    let pool = test_pool(&tmp).await;

    // 建一个源 + 两个组合，源被两个组合共享
    let res = sqlx::query("INSERT INTO sources (url, name, enabled, created_at) VALUES ('ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#S', 's', 1, 'now')")
        .execute(&pool).await.unwrap();
    let src_id = res.last_insert_rowid();
    let res = sqlx::query("INSERT INTO combined_subs (name, created_at) VALUES ('a', 'now')")
        .execute(&pool)
        .await
        .unwrap();
    let ca = res.last_insert_rowid();
    let res = sqlx::query("INSERT INTO combined_subs (name, created_at) VALUES ('b', 'now')")
        .execute(&pool)
        .await
        .unwrap();
    let cb = res.last_insert_rowid();
    for cid in [ca, cb] {
        sqlx::query("INSERT INTO combined_sources (combined_id, source_id) VALUES (?, ?)")
            .bind(cid)
            .bind(src_id)
            .execute(&pool)
            .await
            .unwrap();
    }

    // 删除源 → 两个组合的成员关系都被级联清理
    sqlx::query("DELETE FROM sources WHERE id = ?")
        .bind(src_id)
        .execute(&pool)
        .await
        .unwrap();
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM combined_sources WHERE source_id = ?")
        .bind(src_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0, "source deletion must cascade to combined_sources");

    // 删组合 a → 其成员关系清理（b 不受影响——重新插回源再验证）
    let res = sqlx::query("INSERT INTO sources (url, name, enabled, created_at) VALUES ('ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#S2', 's2', 1, 'now')")
        .execute(&pool).await.unwrap();
    let src2 = res.last_insert_rowid();
    for cid in [ca, cb] {
        sqlx::query("INSERT INTO combined_sources (combined_id, source_id) VALUES (?, ?)")
            .bind(cid)
            .bind(src2)
            .execute(&pool)
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM combined_subs WHERE id = ?")
        .bind(ca)
        .execute(&pool)
        .await
        .unwrap();
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM combined_sources WHERE combined_id = ?")
        .bind(ca)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0, "combined deletion must cascade members");
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM combined_sources WHERE combined_id = ?")
        .bind(cb)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1, "other combined must keep its member");
}

#[tokio::test]
async fn combined_crud_and_members() {
    let tmp = fresh_tmp("combined-crud");
    let pool = test_pool(&tmp).await;
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg).await;
    let admin = setup_admin(&app).await;

    let auth = |mut req: Request<Body>| {
        req.headers_mut().insert(
            "authorization",
            format!("Bearer {}", admin).parse().unwrap(),
        );
        req
    };

    // 两个源
    let srcs: Vec<i64> = {
        let mut v = Vec::new();
        for (name, url) in [
            ("s1", "ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#S1"),
            ("s2", "ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#S2"),
        ] {
            let resp = app
                .clone()
                .oneshot(auth(
                    Request::builder()
                        .method("POST")
                        .uri("/admin/sources")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            json!({"url": url, "name": name, "kind": "single"}).to_string(),
                        ))
                        .unwrap(),
                ))
                .await
                .unwrap();
            let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap();
            let v0: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            v.push(v0["id"].as_i64().unwrap());
        }
        v
    };

    // 创建组合（勾选 s1；s2 不选；另给一个不存在的 id 999 → 忽略）
    let resp = app
        .clone()
        .oneshot(auth(
            Request::builder()
                .method("POST")
                .uri("/admin/combineds")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"name": "my-sub", "source_ids": [srcs[0], 999]}).to_string(),
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
    assert_eq!(v["name"], "my-sub");
    assert_eq!(
        v["source_ids"],
        json!([srcs[0]]),
        "nonexistent source id must be ignored"
    );
    let cid = v["id"].as_i64().unwrap();

    // 列表
    let resp = app
        .clone()
        .oneshot(auth(
            Request::builder()
                .uri("/admin/combineds")
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let list: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["source_ids"], json!([srcs[0]]));

    // 成员全量替换为 [s2]
    let resp = app
        .clone()
        .oneshot(auth(
            Request::builder()
                .method("PUT")
                .uri(format!("/admin/combineds/{}", cid))
                .header("content-type", "application/json")
                .body(Body::from(json!({"source_ids": [srcs[1]]}).to_string()))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        v["source_ids"],
        json!([srcs[1]]),
        "members must be fully replaced"
    );

    // 改名
    let resp = app
        .clone()
        .oneshot(auth(
            Request::builder()
                .method("PUT")
                .uri(format!("/admin/combineds/{}", cid))
                .header("content-type", "application/json")
                .body(Body::from(json!({"name": "renamed-sub"}).to_string()))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["name"], "renamed-sub");

    // 名字冲突 → 400；非法名字 → 400
    let resp = app
        .clone()
        .oneshot(auth(
            Request::builder()
                .method("POST")
                .uri("/admin/combineds")
                .header("content-type", "application/json")
                .body(Body::from(json!({"name": "renamed-sub"}).to_string()))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let resp = app
        .clone()
        .oneshot(auth(
            Request::builder()
                .method("POST")
                .uri("/admin/combineds")
                .header("content-type", "application/json")
                .body(Body::from(json!({"name": "bad name!"}).to_string()))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 删除 → 404（不存在）
    let resp = app
        .clone()
        .oneshot(auth(
            Request::builder()
                .method("DELETE")
                .uri(format!("/admin/combineds/{}", cid))
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let resp = app
        .clone()
        .oneshot(auth(
            Request::builder()
                .method("DELETE")
                .uri(format!("/admin/combineds/{}", cid))
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn combined_subscription_serves_only_members() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/sub"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string("ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#IN\n"),
        )
        .mount(&mock)
        .await;

    let tmp = fresh_tmp("combined-sub");
    let pool = test_pool(&tmp).await;
    let url = format!("{}/sub", mock.uri());
    // remote 源（mock，节点 IN）与 single 源（节点 OUT，指向不可达地址）
    let res = sqlx::query(
        "INSERT INTO sources (url, name, kind, enabled, created_at) VALUES (?, ?, ?, 1, ?)",
    )
    .bind(&url)
    .bind("in-src")
    .bind("remote")
    .bind("now")
    .execute(&pool)
    .await
    .unwrap();
    let in_id = res.last_insert_rowid();
    sqlx::query(
        "INSERT INTO sources (url, name, kind, enabled, created_at) VALUES (?, ?, ?, 1, ?)",
    )
    .bind("ss://YWVzLTI1Ni1nY206cGFzcw@127.0.0.1:1#OUT")
    .bind("out-src")
    .bind("single")
    .bind("now")
    .execute(&pool)
    .await
    .unwrap();

    // 组合只勾选 in-src
    sqlx::query("INSERT INTO combined_subs (name, created_at) VALUES ('grp', 'now')")
        .execute(&pool)
        .await
        .unwrap();
    let cid: i64 = sqlx::query_scalar("SELECT id FROM combined_subs WHERE name = 'grp'")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO combined_sources (combined_id, source_id) VALUES (?, ?)")
        .bind(cid)
        .bind(in_id)
        .execute(&pool)
        .await
        .unwrap();

    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg).await;

    // clash 分支已改订阅组模式（成员过滤在 provider 的 v2ray 聚合链路上），
    // 成员过滤语义改在 v2ray 分支验证
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/subscribe/grp?format=v2ray")
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
    let decoded = b64_decode(&body);
    assert!(decoded.contains("#IN"), "member node must be present");
    assert!(!decoded.contains("OUT"), "non-member must be excluded");
}

#[tokio::test]
async fn combined_subscription_empty_members_returns_200() {
    let tmp = fresh_tmp("combined-empty");
    let pool = test_pool(&tmp).await;
    sqlx::query("INSERT INTO combined_subs (name, created_at) VALUES ('empty-grp', 'now')")
        .execute(&pool)
        .await
        .unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg).await;

    // clash 分支：不拉源，模板输出 200
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/subscribe/empty-grp?format=clash")
                .header("host", "example.com")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "clash empty members must be 200, not 502"
    );

    // v2ray 分支：零成员拉取 → 空 base64 输出 200（覆盖拉源路径的空成员守卫）
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/subscribe/empty-grp?format=v2ray")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "v2ray empty members must be 200, not 502"
    );
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(
        bytes.is_empty(),
        "v2ray empty members must produce empty body"
    );
}

#[tokio::test]
async fn preview_combined_filter() {
    let tmp = fresh_tmp("preview-cmb");
    let pool = test_pool(&tmp).await;
    let res = sqlx::query("INSERT INTO sources (url, name, kind, enabled, created_at) VALUES ('ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#S1', 's1', 'single', 1, 'now')")
        .execute(&pool)
        .await
        .unwrap();
    let src = res.last_insert_rowid();
    sqlx::query("INSERT INTO combined_subs (name, created_at) VALUES ('grp', 'now')")
        .execute(&pool)
        .await
        .unwrap();
    let cid: i64 = sqlx::query_scalar("SELECT id FROM combined_subs WHERE name = 'grp'")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO combined_sources (combined_id, source_id) VALUES (?, ?)")
        .bind(cid)
        .bind(src)
        .execute(&pool)
        .await
        .unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg).await;
    let admin = setup_admin(&app).await;

    let auth = |mut req: Request<Body>| {
        req.headers_mut().insert(
            "authorization",
            format!("Bearer {}", admin).parse().unwrap(),
        );
        req
    };

    // 按组合过滤
    let resp = app
        .clone()
        .oneshot(auth(
            Request::builder()
                .uri("/admin/preview?combined=grp")
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
    assert_eq!(v["total"], 1);
    assert_eq!(v["nodes"][0]["name"], "S1");

    // 不存在的组合 → 404
    let resp = app
        .clone()
        .oneshot(auth(
            Request::builder()
                .uri("/admin/preview?combined=nope")
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // 省略参数 → 全部源
    let resp = app
        .oneshot(auth(
            Request::builder()
                .uri("/admin/preview")
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
    assert_eq!(v["total"], 1);
}

#[tokio::test]
async fn combined_subscription_all_members_failed_returns_502() {
    // 组合的全部成员源都拉取失败（remote 源指向必然拒绝连接的地址）→ 502 附错误明细。
    let tmp = fresh_tmp("combined-502");
    let pool = test_pool(&tmp).await;

    // remote 源：127.0.0.1:1 连接被立即拒绝，fetch_source 报错 → SourceError
    let res = sqlx::query(
        "INSERT INTO sources (url, name, kind, enabled, created_at) VALUES (?, ?, ?, 1, ?)",
    )
    .bind("http://127.0.0.1:1/sub")
    .bind("dead-src")
    .bind("remote")
    .bind("now")
    .execute(&pool)
    .await
    .unwrap();
    let src_id = res.last_insert_rowid();

    // 建组合勾选该源
    sqlx::query("INSERT INTO combined_subs (name, created_at) VALUES ('grp', 'now')")
        .execute(&pool)
        .await
        .unwrap();
    let cid: i64 = sqlx::query_scalar("SELECT id FROM combined_subs WHERE name = 'grp'")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO combined_sources (combined_id, source_id) VALUES (?, ?)")
        .bind(cid)
        .bind(src_id)
        .execute(&pool)
        .await
        .unwrap();

    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg).await;

    // clash 分支已改订阅组模式（不拉源），全源失败的 502 语义在 v2ray 分支
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/subscribe/grp?format=v2ray")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["error"]["code"], "bad_gateway");
    let msg = v["error"]["message"].as_str().unwrap();
    assert!(
        msg.contains("dead-src"),
        "error detail must name the failed source, got: {msg}"
    );
}

#[tokio::test]
async fn user_and_session_db_functions() {
    let tmp = fresh_tmp("auth-db");
    let pool = test_pool(&tmp).await;

    // 初始无用户
    assert!(server::db::users_empty(&pool).await.unwrap());

    // 创建 + 验证
    server::db::create_user(&pool, "admin", "correct-password")
        .await
        .unwrap();
    assert!(!server::db::users_empty(&pool).await.unwrap());
    assert!(
        server::db::verify_user(&pool, "admin", "correct-password")
            .await
            .unwrap()
    );
    assert!(
        !server::db::verify_user(&pool, "admin", "wrong-password")
            .await
            .unwrap()
    );
    assert!(
        !server::db::verify_user(&pool, "nobody", "correct-password")
            .await
            .unwrap()
    );
    assert_eq!(
        server::db::get_username(&pool).await.unwrap().as_deref(),
        Some("admin")
    );

    // 重名 → Err
    assert!(server::db::create_user(&pool, "admin", "x").await.is_err());

    // 会话生命周期
    let t1 = server::db::create_session(&pool).await.unwrap();
    let t2 = server::db::create_session(&pool).await.unwrap();
    assert_ne!(t1, t2);
    assert_eq!(t1.len(), 64); // 32 bytes hex
    assert!(server::db::validate_session(&pool, &t1, 0).await.unwrap());
    assert!(
        !server::db::validate_session(&pool, "f".repeat(64).as_str(), 0)
            .await
            .unwrap()
    );
    server::db::delete_session(&pool, &t1).await.unwrap();
    assert!(!server::db::validate_session(&pool, &t1, 0).await.unwrap());
    server::db::delete_all_sessions(&pool).await.unwrap();
    assert!(!server::db::validate_session(&pool, &t2, 0).await.unwrap());

    // TTL 滑动过期：last_used_at 回拨超过 TTL → 过期删除；回拨在 TTL 内 → 续期有效。
    // 回拨用 strftime 输出 RFC3339（datetime('now') 是 'YYYY-MM-DD HH:MM:SS' 空格格式，
    // chrono 的 parse_from_rfc3339 无法解析会走"无法解析视为过期"分支，测不到真正的 TTL 判断）。
    let t3 = server::db::create_session(&pool).await.unwrap();
    sqlx::query("UPDATE sessions SET last_used_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-40 days') WHERE token_hash = ?")
        .bind(sha256_hex_manual(&t3))
        .execute(&pool).await.unwrap();
    assert!(
        !server::db::validate_session(&pool, &t3, 30).await.unwrap(),
        "过期会话必须失效"
    );
    assert!(
        !server::db::validate_session(&pool, &t3, 30).await.unwrap(),
        "过期会话已被惰性删除"
    );
    // 回拨 10 天（TTL=30 内）→ 滑动续期后有效
    let t4 = server::db::create_session(&pool).await.unwrap();
    sqlx::query("UPDATE sessions SET last_used_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-10 days') WHERE token_hash = ?")
        .bind(sha256_hex_manual(&t4))
        .execute(&pool).await.unwrap();
    assert!(
        server::db::validate_session(&pool, &t4, 30).await.unwrap(),
        "TTL 内会话有效（滑动续期）"
    );
    // TTL=0 禁用过期
    let t5 = server::db::create_session(&pool).await.unwrap();
    sqlx::query("UPDATE sessions SET last_used_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-100 days') WHERE token_hash = ?")
        .bind(sha256_hex_manual(&t5))
        .execute(&pool).await.unwrap();
    assert!(
        server::db::validate_session(&pool, &t5, 0).await.unwrap(),
        "ttl=0 禁用过期"
    );

    // 超大 TTL（u64 超 i64::MAX）：不 panic、不过期（永不过期语义）
    let t6 = server::db::create_session(&pool).await.unwrap();
    sqlx::query(
        "UPDATE sessions SET last_used_at = datetime('now', '-100 days') WHERE token_hash = ?",
    )
    .bind(sha256_hex_manual(&t6))
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        server::db::validate_session(&pool, &t6, u64::MAX)
            .await
            .unwrap(),
        "ttl 超 i64::MAX 视为永不过期"
    );
    assert!(
        server::db::delete_expired_sessions(&pool, u64::MAX)
            .await
            .is_ok(),
        "超限 ttl 清理 no-op 不 panic"
    );

    // i64::MAX（在 chrono Duration::days 溢出带内）：try_days None → 永不过期语义
    assert!(
        server::db::validate_session(&pool, &t6, i64::MAX as u64)
            .await
            .unwrap(),
        "ttl=i64::MAX 不得 panic 且视为永不过期"
    );
    assert!(
        server::db::delete_expired_sessions(&pool, i64::MAX as u64)
            .await
            .is_ok(),
        "i64::MAX ttl 清理 no-op 不 panic"
    );

    // 写入秒精度（无纳秒小数）：新会话 last_used_at 为 'YYYY-MM-DDTHH:MM:SSZ' 形态
    let t7 = server::db::create_session(&pool).await.unwrap();
    let stored: String =
        sqlx::query_scalar("SELECT last_used_at FROM sessions WHERE token_hash = ?")
            .bind(sha256_hex_manual(&t7))
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        !stored.contains('.'),
        "session last_used_at must be second precision: {stored}"
    );

    // update_password 对不存在用户必须报错（不再静默 no-op）
    assert!(
        server::db::update_password(&pool, "nobody", "x-password")
            .await
            .is_err(),
        "update_password on missing user must error"
    );

    // 改密码后旧密码失效、新密码生效
    server::db::update_password(&pool, "admin", "new-password")
        .await
        .unwrap();
    assert!(
        !server::db::verify_user(&pool, "admin", "correct-password")
            .await
            .unwrap()
    );
    assert!(
        server::db::verify_user(&pool, "admin", "new-password")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn setup_creates_admin_once() {
    let tmp = fresh_tmp("setup");
    let pool = test_pool(&tmp).await;
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg).await;

    // setup-status：未创建 → needs_setup true
    let (s, v) = http(&app, "GET", "/admin/setup-status", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["needs_setup"], serde_json::Value::Bool(true));

    // 创建成功 → 200
    let (s, v) = http(
        &app,
        "POST",
        "/admin/setup",
        Some(valid_setup("admin")),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["username"], "admin");

    // 已创建：setup-status false + setup 409 锁定
    let (_, v) = http(&app, "GET", "/admin/setup-status", None, None).await;
    assert_eq!(v["needs_setup"], serde_json::Value::Bool(false));
    let (s, _) = http(
        &app,
        "POST",
        "/admin/setup",
        Some(valid_setup("admin2")),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
}

#[tokio::test]
async fn setup_validates_fields() {
    let tmp = fresh_tmp("setup-val");
    let pool = test_pool(&tmp).await;
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg).await;

    // 密码过短 → 400
    let (s, _) = http(
        &app,
        "POST",
        "/admin/setup",
        Some(
            json!({"username": "a", "password": "short", "password_confirm": "short"}).to_string(),
        ),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // 两次不一致 → 400
    let (s, _) = http(
        &app,
        "POST",
        "/admin/setup",
        Some(
            json!({"username": "a", "password": "pass-12345", "password_confirm": "different"})
                .to_string(),
        ),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // 非法用户名 → 400
    let (s, _) = http(&app, "POST", "/admin/setup",
        Some(json!({"username": "bad name!", "password": "pass-12345", "password_confirm": "pass-12345"}).to_string()), None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn login_and_logout_flow() {
    let tmp = fresh_tmp("login");
    let pool = test_pool(&tmp).await;
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg).await;

    http(
        &app,
        "POST",
        "/admin/setup",
        Some(valid_setup("admin")),
        None,
    )
    .await;

    // 正确凭证 → 200 + token（64 hex）
    let (s, v) = http(
        &app,
        "POST",
        "/admin/login",
        Some(json!({"username": "admin", "password": "pass-12345"}).to_string()),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let token = v["token"].as_str().unwrap().to_string();
    assert_eq!(token.len(), 64);
    assert!(
        token.chars().all(|c| c.is_ascii_hexdigit()),
        "token must be hex: {token}"
    );

    // 错误密码 / 不存在用户 → 统一 401
    for body in [
        json!({"username": "admin", "password": "wrong"}).to_string(),
        json!({"username": "nobody", "password": "pass-12345"}).to_string(),
    ] {
        let (s, v) = http(&app, "POST", "/admin/login", Some(body), None).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        assert_eq!(v["error"]["code"], "unauthorized");
    }

    // logout 删除会话：同一 token 再访问受保护端点 → 401
    let (s, _) = http(&app, "POST", "/admin/logout", None, Some(&token)).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _) = http(&app, "GET", "/admin/config", None, Some(&token)).await;
    assert_eq!(
        s,
        StatusCode::UNAUTHORIZED,
        "logout must delete the session"
    );

    // logout 幂等：token 已失效仍返回 204
    let (s, _) = http(&app, "POST", "/admin/logout", None, Some(&token)).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn expired_session_rejected_by_api() {
    // TTL 生效的端到端路径：会话超期后任何受保护请求必须 401。
    let tmp = fresh_tmp("ttl");
    let pool = test_pool(&tmp).await;
    let cfg = test_config(&tmp); // session_ttl_days: 0 基线上覆盖
    let app = server::routes::build_router(
        pool.clone(),
        AppConfig {
            session_ttl_days: 30,
            ..cfg
        },
    )
    .await;
    let admin = setup_admin(&app).await;
    // 回拨 last_used_at 超 TTL（RFC3339 格式，见 user_and_session_db_functions 注释）→ 请求 401
    sqlx::query(
        "UPDATE sessions SET last_used_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-40 days')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let (s, _) = http(&app, "GET", "/admin/config", None, Some(&admin)).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "过期会话必须 401");
}

#[tokio::test]
async fn preview_filters_by_kind() {
    let tmp = fresh_tmp("preview-kind");
    let pool = test_pool(&tmp).await;
    // single 源（不拉网络）+ remote 源（指向 127.0.0.1:1 必然失败 → 产生源错误但请求 200）
    sqlx::query("INSERT INTO sources (url, name, kind, enabled, created_at) VALUES ('ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#LOCAL', 'local', 'single', 1, 'now')")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO sources (url, name, kind, enabled, created_at) VALUES ('http://127.0.0.1:1/sub', 'dead', 'remote', 1, 'now')")
        .execute(&pool).await.unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg).await;
    let admin = setup_admin(&app).await;

    // ?kind=single → 只有 local 源节点
    let (s, v) = http(
        &app,
        "GET",
        "/admin/preview?kind=single",
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["total"], 1);
    assert_eq!(v["nodes"][0]["name"], "LOCAL");

    // ?kind=remote → 无节点但 200（源失败进 errors）
    let (s, v) = http(
        &app,
        "GET",
        "/admin/preview?kind=remote",
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["total"], 0);
    assert!(
        !v["errors"].as_array().unwrap().is_empty(),
        "dead remote 源应进 errors"
    );

    // kind + combined 互斥 → 400
    let (s, _) = http(
        &app,
        "GET",
        "/admin/preview?kind=single&combined=grp",
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // 非法 kind → 400
    let (s, _) = http(&app, "GET", "/admin/preview?kind=bogus", None, Some(&admin)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn setup_is_atomic_against_duplicate_admin() {
    // 回归：并发/重复 setup 不产生第二个管理员（INSERT...SELECT WHERE NOT EXISTS 原子性）。
    let tmp = fresh_tmp("setup-atomic");
    let pool = test_pool(&tmp).await;
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg).await;

    let (s, _) = http(
        &app,
        "POST",
        "/admin/setup",
        Some(valid_setup("admin")),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    // 已存在时再 setup（不同用户名）→ 409（原子 INSERT 的 affected=0 分支）
    let (s, _) = http(
        &app,
        "POST",
        "/admin/setup",
        Some(valid_setup("admin2")),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    // 确认只有一个用户
    let pool2 = test_pool(&tmp).await; // 复用同一 db 文件重新连接验证
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&pool2)
        .await
        .unwrap();
    assert_eq!(n, 1, "must never have two admins");
}

#[tokio::test]
async fn concurrent_setup_never_creates_two_admins() {
    // 两个 setup 并发（不同用户名）：原子 INSERT 保证只有一个成功
    let tmp = fresh_tmp("setup-conc");
    let pool = test_pool(&tmp).await;
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool.clone(), cfg).await;

    let app1 = app.clone();
    let app2 = app.clone();
    let (r1, r2) = tokio::join!(
        http(
            &app1,
            "POST",
            "/admin/setup",
            Some(valid_setup("admin-a")),
            None
        ),
        http(
            &app2,
            "POST",
            "/admin/setup",
            Some(valid_setup("admin-b")),
            None
        ),
    );
    let ok_count = [r1.0, r2.0]
        .iter()
        .filter(|s| **s == StatusCode::OK)
        .count();
    assert_eq!(ok_count, 1, "exactly one setup must succeed: {r1:?} {r2:?}");
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
async fn clash_config_get_put_and_subscription_output() {
    let tmp = fresh_tmp("clash-cfg");
    let pool = test_pool(&tmp).await;
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool.clone(), cfg).await;
    let admin = setup_admin(&app).await;

    // GET 缺省返回默认模板
    let (s, v) = http(&app, "GET", "/admin/clash-config", None, Some(&admin)).await;
    assert_eq!(s, StatusCode::OK);
    let tpl = v["template"].as_str().unwrap().to_string();
    assert!(tpl.contains("mixed-port: 7890"), "默认模板含固定头部");

    // PUT 非法 YAML → 400
    let (s, _) = http(
        &app,
        "PUT",
        "/admin/clash-config",
        Some(json!({"template": ": : :"}).to_string()),
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // PUT 合法模板 → 保存成功，回读一致
    let custom = "mode: rule\ndns:\n  enable: true\n  nameserver:\n    - 1.1.1.1\nrules:\n  - MATCH,🚀 节点选择\n";
    let (s, v) = http(
        &app,
        "PUT",
        "/admin/clash-config",
        Some(json!({"template": custom}).to_string()),
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["template"].as_str().unwrap(), custom);
    let (_, v) = http(&app, "GET", "/admin/clash-config", None, Some(&admin)).await;
    assert_eq!(v["template"].as_str().unwrap(), custom, "保存后回读一致");

    // 无鉴权 → 401
    let (s, _) = http(&app, "GET", "/admin/clash-config", None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // 建组合 + 订阅 → clash 输出为订阅组模式（含 providers + 自定义 dns + use 引用）
    sqlx::query("INSERT INTO sources (url, name, kind, enabled, created_at) VALUES ('ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#A', 's1', 'single', 1, 'now')")
        .execute(&pool).await.unwrap();
    let src_id: i64 = sqlx::query_scalar("SELECT id FROM sources WHERE name = 's1'")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO combined_subs (name, created_at) VALUES ('grp', 'now')")
        .execute(&pool)
        .await
        .unwrap();
    let cid: i64 = sqlx::query_scalar("SELECT id FROM combined_subs WHERE name = 'grp'")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO combined_sources (combined_id, source_id) VALUES (?, ?)")
        .bind(cid)
        .bind(src_id)
        .execute(&pool)
        .await
        .unwrap();

    let (s, body) = http_raw(&app, "GET", "/subscribe/grp?format=clash", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.contains("proxy-providers:"), "订阅组模式输出");
    assert!(body.contains("grp:"), "provider key = 组合名");
    assert!(
        body.contains("url: http://example.com/subscribe/grp?format=v2ray"),
        "provider url 拼请求 Host"
    );
    assert!(body.contains("use:"));
    assert!(body.contains("- grp"));
    assert!(body.contains("dns:"), "自定义模板段保留");
    assert!(!body.contains("proxies:\n  - name:"), "不再输出解析节点");
}

#[tokio::test]
async fn subscribe_clash_without_host_returns_400() {
    let tmp = fresh_tmp("clash-nohost");
    let pool = test_pool(&tmp).await;
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool.clone(), cfg).await;
    // 建空组合
    sqlx::query("INSERT INTO combined_subs (name, created_at) VALUES ('grp', 'now')")
        .execute(&pool)
        .await
        .unwrap();
    // 无 Host header 的请求
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/subscribe/grp?format=clash")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
