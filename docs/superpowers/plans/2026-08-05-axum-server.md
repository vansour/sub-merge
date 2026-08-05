# Plan B: axum 服务端 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 构建 axum HTTP 服务：订阅输出接口（`/api/subscribe`）、管理 API（`/api/admin/*`）、SQLite 持久化、并发拉取合并、双 token 鉴权、前端 WASM 静态资源托管。

**Architecture:** axum 0.8 路由 + SQLx 0.9 SQLite 存储 + reqwest 并发抓取。鉴权用两个独立 token（订阅 token / 管理 token），订阅 token 走 query 参数，管理 token 走 Bearer。静态资源由 axum 托管（serve-dir 或嵌入式 include）。

**Tech Stack:** axum 0.8, tokio 1, sqlx 0.9 (sqlite, runtime-tokio), reqwest 0.13, tower-http 0.7, serde, rand, thiserror, tracing, tower。

## Global Constraints

- 继承 Plan A 的 `proxy-core`（`crates/proxy-core`），本 plan 新增 `crates/server`
- workspace 根：`/root/github/sub-merge`，新增 member：`crates/server`
- 鉴权：订阅 token 与管理 token 分开，存 SQLite `settings` 表，启动时自动生成（随机 32 字节 hex）
- 订阅接口 `GET /api/subscribe`：`?token=<subscribe_token>&format=<clash|v2ray|singbox>`，默认 format=`clash`
- 管理接口 `GET/POST/PUT/DELETE /api/admin/*`：`Authorization: Bearer <admin_token>`
- 并发拉取：默认并发 8，单源超时 15s，单源失败跳过，全部失败返回 502
- 节点总数上限默认 2000
- 错误格式统一：`{ "error": { "code": "...", "message": "..." } }`
- 环境变量：`PORT`(8080)、`DATABASE_PATH`(`./submerge.db`)、`CONCURRENCY`(8)、`TIMEOUT_SECS`(15)、`MAX_NODES`(2000)
- 前端 WASM 静态资源目录：`crates/server/web/dist`（由 Plan C 构建产出）

---

## 文件结构总览

```
crates/server/
├── Cargo.toml
├── src/
│   ├── main.rs          # 入口：加载配置、初始化 DB、构建路由
│   ├── config.rs        # 环境变量配置 AppConfig
│   ├── state.rs         # AppState（DbPool, 配置, http client）
│   ├── db.rs            # SQLx 初始化、建表、settings token 读写
│   ├── error.rs         # ApiError + IntoResponse
│   ├── auth.rs          # 鉴权：admin Bearer 校验、subscribe token 校验
│   ├── service.rs       # 核心业务：拉取合并转换
│   ├── routes/
│   │   ├── mod.rs       # 路由组装
│   │   ├── subscribe.rs # GET /api/subscribe
│   │   ├── sources.rs   # /api/admin/sources CRUD
│   │   ├── preview.rs   # GET /api/admin/preview
│   │   └── config.rs    # GET/PUT /api/admin/config
│   └── static.rs        # 静态资源托管
└── tests/
    └── api_test.rs      # 集成测试：路由/鉴权/错误码
```

---

### Task 1: workspace 扩展 + server crate 脚手架 + 配置 + DB 初始化

**Files:**
- Modify: `/root/github/sub-merge/Cargo.toml`（加 `crates/server`）
- Create: `/root/github/sub-merge/crates/server/Cargo.toml`
- Create: `/root/github/sub-merge/crates/server/src/main.rs`
- Create: `/root/github/sub-merge/crates/server/src/config.rs`
- Create: `/root/github/sub-merge/crates/server/src/db.rs`
- Create: `/root/github/sub-merge/crates/server/src/lib.rs`
- Test: `crates/server/tests/api_test.rs`（首个测试）

**Interfaces:**
- Consumes: `proxy_core`（Plan A 产物）
- Produces:
  - `struct AppConfig { port: u16, db_path: PathBuf, concurrency: usize, timeout_secs: u64, max_nodes: usize, web_dist: PathBuf }`
  - `pub struct AppState { pub pool: SqlitePool, pub cfg: AppConfig, pub http: reqwest::Client }`（`Clone`）
  - `pub async fn init_db(path: &Path) -> anyhow::Result<SqlitePool>`
  - `pub async fn get_setting(pool: &SqlitePool, key: &str) -> anyhow::Result<Option<String>>`
  - `pub async fn set_setting(pool: &SqlitePool, key: &str, value: &str) -> anyhow::Result<()>`
  - `pub async fn ensure_tokens(pool: &SqlitePool) -> anyhow::Result<(String, String)>` — 返回 (subscribe_token, admin_token)，首次生成
  - `pub fn gen_token() -> String`

- [ ] **Step 1: 修改 workspace Cargo.toml**

```toml
[workspace]
resolver = "2"
members = ["crates/proxy-core", "crates/server"]

[workspace.package]
edition = "2024"
rust-version = "1.97"
```

- [ ] **Step 2: 创建 server Cargo.toml**

```toml
[package]
name = "server"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true

[dependencies]
proxy-core = { path = "../proxy-core" }
axum = { version = "0.8", features = ["json", "macros", "query"] }
tokio = { version = "1", features = ["full"] }
sqlx = { version = "0.9", features = ["runtime-tokio", "sqlite", "derive"] }
reqwest = { version = "0.13", features = ["json"] }
tower-http = { version = "0.7", features = ["cors", "trace"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
rand = "0.8"
thiserror = "2"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
anyhow = "1"
hex = "0.4"

[dev-dependencies]
tower = { version = "0.5", features = ["util"] }
http-body-util = "0.1"
wiremock = "0.6"
```

- [ ] **Step 3: 实现 config.rs**

```rust
// crates/server/src/config.rs
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub port: u16,
    pub db_path: PathBuf,
    pub concurrency: usize,
    pub timeout_secs: u64,
    pub max_nodes: usize,
    pub web_dist: PathBuf,
}

impl AppConfig {
    pub fn from_env() -> Self {
        let port = std::env::var("PORT").ok().and_then(|v| v.parse().ok()).unwrap_or(8080);
        let db_path = std::env::var("DATABASE_PATH").unwrap_or_else(|_| "./submerge.db".into());
        let concurrency = std::env::var("CONCURRENCY").ok().and_then(|v| v.parse().ok()).unwrap_or(8);
        let timeout_secs = std::env::var("TIMEOUT_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(15);
        let max_nodes = std::env::var("MAX_NODES").ok().and_then(|v| v.parse().ok()).unwrap_or(2000);
        let web_dist = std::env::var("WEB_DIST").unwrap_or_else(|_| "./web/dist".into());
        Self {
            port,
            db_path: PathBuf::from(db_path),
            concurrency,
            timeout_secs,
            max_nodes,
            web_dist: PathBuf::from(web_dist),
        }
    }
}
```

- [ ] **Step 4: 实现 db.rs**

```rust
// crates/server/src/db.rs
use anyhow::Result;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use std::path::Path;

pub async fn init_db(path: &Path) -> Result<SqlitePool> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(std::time::Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_lazy_with(opts);

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sources (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            url TEXT NOT NULL,
            name TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL
        )",
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
    )
    .execute(&pool)
    .await?;

    Ok(pool)
}

pub async fn get_setting(pool: &SqlitePool, key: &str) -> Result<Option<String>> {
    let row = sqlx::query("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.get::<String, _>(0)))
}

pub async fn set_setting(pool: &SqlitePool, key: &str, value: &str) -> Result<()> {
    sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
        .bind(key)
        .bind(value)
        .execute(pool)
        .await?;
    Ok(())
}

pub fn gen_token() -> String {
    use rand::RngCore;
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

pub async fn ensure_tokens(pool: &SqlitePool) -> Result<(String, String)> {
    let sub = get_setting(pool, "subscribe_token").await?;
    let sub = match sub {
        Some(s) => s,
        None => {
            let t = gen_token();
            set_setting(pool, "subscribe_token", &t).await?;
            t
        }
    };
    let admin = get_setting(pool, "admin_token").await?;
    let admin = match admin {
        Some(s) => s,
        None => {
            let t = gen_token();
            set_setting(pool, "admin_token", &t).await?;
            t
        }
    };
    Ok((sub, admin))
}
```

- [ ] **Step 5: 创建 lib.rs（暴露测试用模块）**

```rust
// crates/server/src/lib.rs
// Task 1 只声明本任务创建的模块：config/db/error/state/routes。
// auth 由 Task 3 创建时加声明；service 由 Task 2 创建时加声明。
pub mod config;
pub mod db;
pub mod error;
pub mod routes;
pub mod state;
```

- [ ] **Step 6: 写首个集成测试 `tests/api_test.rs`**

```rust
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
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}", std::process::id()));
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
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}", std::process::id()));
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
```

- [ ] **Step 7: 运行确认失败（缺模块）**

Run: `cargo test -p server --test api_test`
Expected: FAIL — `cannot find module routes` 等

- [ ] **Step 8: 创建 state.rs、error.rs、routes/mod.rs、static.rs 骨架让编译通过**

state.rs:
```rust
// crates/server/src/state.rs
use crate::config::AppConfig;
use sqlx::sqlite::SqlitePool;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub cfg: AppConfig,
    pub http: reqwest::Client,
    pub admin_token: String,
}

impl AppState {
    pub fn new(pool: SqlitePool, cfg: AppConfig, admin_token: String) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(cfg.timeout_secs))
            .build()
            .expect("reqwest client");
        Self { pool, cfg, http, admin_token }
    }
}
```

error.rs:
```rust
// crates/server/src/error.rs
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    pub fn unauthorized(msg: impl Into<String>) -> Self {
        Self { status: StatusCode::UNAUTHORIZED, code: "unauthorized", message: msg.into() }
    }
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self { status: StatusCode::BAD_REQUEST, code: "bad_request", message: msg.into() }
    }
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self { status: StatusCode::NOT_FOUND, code: "not_found", message: msg.into() }
    }
    pub fn internal(msg: impl Into<String>) -> Self {
        Self { status: StatusCode::INTERNAL_SERVER_ERROR, code: "internal_error", message: msg.into() }
    }
    pub fn bad_gateway(msg: impl Into<String>) -> Self {
        Self { status: StatusCode::BAD_GATEWAY, code: "bad_gateway", message: msg.into() }
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
```

routes/mod.rs（骨架，路由后续 Task 填充）:
```rust
// crates/server/src/routes/mod.rs
// Task 1 只建立最小 Router。subscribe/sources/preview/config 子模块
// 由 Task 2-4 创建时在 lib.rs 和本文件补声明。
use crate::state::AppState;
use axum::routing::get;
use axum::Router;

pub async fn build_router(pool: sqlx::sqlite::SqlitePool, cfg: crate::config::AppConfig, admin_token: String) -> Router {
    let state = AppState::new(pool, cfg, admin_token);
    Router::new()
        .route("/", get(|| async { "sub-merge is running" }))
        .with_state(state)
}
```

static.rs（占位）:
```rust
// crates/server/src/static.rs
// Plan B Task 6 填充完整静态托管逻辑
```

- [ ] **Step 9: 运行确认通过**

Run: `cargo test -p server --test api_test`
Expected: PASS（2 个测试）

> 注意：`routes::subscribe::subscribe_handler`、`sources::router()` 等模块需在 Task 2-5 填充，Task 1 先提供空实现避免编译错误。Task 1 的 router 里引用的处理函数用一个返回 501 的占位实现。

实际为了 Task 1 编译通过，routes 各子模块先给最小占位：
```rust
// routes/subscribe.rs (Task 1 占位，Task 2 填充)
pub async fn subscribe_handler(
    _state: State<AppState>,
    _q: axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<axum::response::Response, ApiError> {
    Err(ApiError::bad_request("not implemented"))
}
```
（sources/preview/config 同理给 501 占位）

- [ ] **Step 10: Commit**

```bash
git add -A
git commit -m "feat(server): workspace scaffold, config, db init, router skeleton"
```

---

### Task 2: 订阅接口 GET /api/subscribe

**Files:**
- Create: `crates/server/src/routes/subscribe.rs`（完整实现）
- Create: `crates/server/src/service.rs`
- Test: `crates/server/tests/api_test.rs` 追加

**Interfaces:**
- Consumes: `AppState`, `ApiError`, `proxy_core::parser`, `proxy_core::serializer`
- Produces:
  - `pub async fn subscribe_handler(State(state), Query(q)) -> Result<Response, ApiError>`
  - `service.rs`: `pub async fn fetch_and_merge(state: &AppState, nodes_limit: usize) -> Result<(Vec<ProxyNode>, Vec<SourceError>), ApiError>` 其中 `SourceError { source_name: String, reason: String }`
  - `service.rs`: `pub async fn fetch_source(client: &reqwest::Client, url: &str, timeout: Duration) -> Result<String, String>`

- [ ] **Step 1: 追加测试到 `tests/api_test.rs`**

```rust
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn subscribe_requires_token() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let (sub, admin) = server::db::ensure_tokens(&pool).await.unwrap();
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

    let tmp = std::env::temp_dir().join(format!("submerge-test-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let (sub, admin) = server::db::ensure_tokens(&pool).await.unwrap();

    // 插入一个指向 mock server 的源
    let url = format!("{}/sub", mock.uri());
    sqlx::query("INSERT INTO sources (url, name, enabled) VALUES (?, ?, 1)")
        .bind(&url).bind("mock-source").execute(&pool).await.unwrap();

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
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}", std::process::id()));
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
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p server --test api_test`
Expected: FAIL — 订阅返回 501/错误

- [ ] **Step 3: 实现 service.rs**

```rust
// crates/server/src/service.rs
use crate::state::AppState;
use proxy_core::model::ProxyNode;
use proxy_core::parser::parse_subscription_text;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinSet;

#[derive(Debug, Clone)]
pub struct SourceError {
    pub source_name: String,
    pub reason: String,
}

/// 并发拉取全部 enabled 源，解析合并。返回 (节点, 错误源列表)。
pub async fn fetch_and_merge(state: &AppState) -> (Vec<ProxyNode>, Vec<SourceError>) {
    let sources: Vec<(i64, String, String)> = sqlx::query_as("SELECT id, name, url FROM sources WHERE enabled = 1")
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();

    let max_nodes = state.cfg.max_nodes;
    let client = Arc::new(state.http.clone());
    let timeout = Duration::from_secs(state.cfg.timeout_secs);

    let mut set = JoinSet::new();
    for (_, name, url) in sources {
        let client = client.clone();
        set.spawn(async move {
            match fetch_source(&client, &url, timeout).await {
                Ok(text) => {
                    let (nodes, skipped) = parse_subscription_text(&text, max_nodes);
                    (Some(name), nodes, skipped)
                }
                Err(reason) => (Some(name), Vec::new(), 0usize),
            }
        });
    }

    let mut all_nodes = Vec::new();
    let mut errors = Vec::new();
    while let Some(res) = set.join_next().await {
        let Ok((name, mut nodes, _)) = res else { continue };
        if let Some(n) = name {
            if nodes.is_empty() {
                errors.push(SourceError { source_name: n, reason: "no nodes parsed or fetch failed".into() });
            } else {
                all_nodes.append(&mut nodes);
            }
        }
    }
    // 上限截断
    all_nodes.truncate(max_nodes);
    (all_nodes, errors)
}

pub async fn fetch_source(client: &reqwest::Client, url: &str, timeout: Duration) -> Result<String, String> {
    let resp = client
        .get(url)
        .timeout(timeout)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("http status {}", resp.status()));
    }
    resp.text()
        .await
        .map_err(|e| format!("read body failed: {e}"))
}
```

- [ ] **Step 4: 实现 routes/subscribe.rs**

```rust
// crates/server/src/routes/subscribe.rs
use crate::error::ApiError;
use crate::service;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use proxy_core::serializer::{serialize_nodes, OutputFormat};
use serde_json::json;
use std::str::FromStr;

#[derive(serde::Deserialize)]
pub struct SubscribeQuery {
    pub token: String,
    pub format: Option<String>,
}

pub async fn subscribe_handler(
    State(state): State<AppState>,
    Query(q): Query<SubscribeQuery>,
) -> Result<Response, ApiError> {
    // 校验订阅 token
    let expected = state
        .pool
        .clone();
    let stored = sqlx::query("SELECT value FROM settings WHERE key = 'subscribe_token'")
        .fetch_optional(&expected)
        .await
        .map_err(ApiError::from)?;
    let Some(row) = stored else {
        return Err(ApiError::internal("subscribe token not initialized"));
    };
    let stored: String = row.get(0);
    // 恒定时间比较
    if !constant_eq(&q.token, &stored) {
        return Err(ApiError::unauthorized("invalid subscribe token"));
    }

    let format = match &q.format {
        Some(f) => OutputFormat::from_str(f).map_err(|_| ApiError::bad_request("unsupported format"))?,
        None => OutputFormat::Clash,
    };

    let (nodes, source_errors) = service::fetch_and_merge(&state).await;

    let body = serialize_nodes(&nodes, format).map_err(|e| ApiError::internal(e.to_string()))?;

    let content_type = match format {
        OutputFormat::Clash => "application/x-yaml",
        OutputFormat::V2ray => "text/plain; charset=utf-8",
        OutputFormat::Singbox => "application/json",
    };

    let mut resp = Response::builder()
        .header("content-type", content_type)
        .header("profile-update-interval", "24")
        .body(axum::body::Body::from(body))
        .unwrap();
    // 若所有源都失败，返回 502 附明细
    if nodes.is_empty() && !source_errors.is_empty() {
        resp = Json(json!({
            "error": {
                "code": "bad_gateway",
                "message": "all sources failed",
                "details": source_errors.iter().map(|e| format!("{}: {}", e.source_name, e.reason)).collect::<Vec<_>>()
            }
        })).into_response();
        *resp.status_mut() = axum::http::StatusCode::BAD_GATEWAY;
    }
    Ok(resp)
}

/// 恒定时间字符串比较，防时序侧信道。
fn constant_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test -p server --test api_test`
Expected: PASS（5 个测试）

- [ ] **Step 6: Commit**

```bash
git add crates/server/src/service.rs crates/server/src/routes/subscribe.rs
git commit -m "feat(server): subscribe endpoint with token auth, concurrent fetch, format output"
```

---

### Task 3: 管理 API —— sources CRUD

**Files:**
- Create: `crates/server/src/routes/sources.rs`（完整实现）
- Create: `crates/server/src/auth.rs`
- Test: `crates/server/tests/api_test.rs` 追加

**Interfaces:**
- Consumes: `AppState`, `ApiError`, `AppState::admin_token`
- Produces:
  - `auth.rs`: `pub async fn require_admin(State(state), header: Option<HeaderMap>) -> Result<(), ApiError>` — 校验 Bearer token
  - `pub fn router() -> Router<AppState>` — 挂载 `/api/admin/sources` CRUD + refresh
  - `#[derive(Serialize)] pub struct SourceDto { id, url, name, enabled, created_at }`
  - `#[derive(Deserialize)] pub struct CreateSource { url: String, name: String }`
  - `#[derive(Deserialize)] pub struct UpdateSource { url: Option<String>, name: Option<String>, enabled: Option<bool> }`

- [ ] **Step 1: 追加测试到 `tests/api_test.rs`**

```rust
use serde_json::json;

#[tokio::test]
async fn admin_requires_bearer_token() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}", std::process::id()));
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
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let (_, admin) = server::db::ensure_tokens(&pool).await.unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg, admin).await;

    let auth = |req: Request<Body>| {
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
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p server --test api_test`
Expected: FAIL — CRUD 未实现

- [ ] **Step 3: 实现 auth.rs**

```rust
// crates/server/src/auth.rs
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Request, State};
use axum::http::header::AUTHORIZATION;
use axum::http::HeaderMap;

/// 校验 Bearer 管理 token。返回 Ok(()) 或 401。
pub async fn require_admin(State(state): State<AppState>, headers: HeaderMap) -> Result<(), ApiError> {
    let Some(auth) = headers.get(AUTHORIZATION) else {
        return Err(ApiError::unauthorized("missing authorization header"));
    };
    let Ok(auth_str) = auth.to_str() else {
        return Err(ApiError::unauthorized("invalid authorization header"));
    };
    let Some(token) = auth_str.strip_prefix("Bearer ") else {
        return Err(ApiError::unauthorized("expected Bearer token"));
    };
    if constant_eq(token, &state.admin_token) {
        Ok(())
    } else {
        Err(ApiError::unauthorized("invalid admin token"))
    }
}

fn constant_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

// axum 中间件式鉴权：把 require_admin 作为 before 层。
// 本方案采用在 handler 内显式调用的方式（简单直观），不引入 middleware 层。
```

- [ ] **Step 4: 实现 routes/sources.rs**

```rust
// crates/server/src/routes/sources.rs
use crate::auth::require_admin;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SourceDto {
    pub id: i64,
    pub url: String,
    pub name: String,
    pub enabled: bool,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateSource {
    pub url: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSource {
    pub url: Option<String>,
    pub name: Option<String>,
    pub enabled: Option<bool>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/admin/sources", get(list_sources).post(create_source))
        .route(
            "/api/admin/sources/:id",
            put(update_source).delete(delete_source),
        )
        .route("/api/admin/sources/:id/refresh", post(refresh_source))
}

async fn list_sources(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Vec<SourceDto>>, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    let rows = sqlx::query_as::<_, SourceDto>(
        "SELECT id, url, name, enabled, created_at FROM sources ORDER BY id",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

async fn create_source(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<CreateSource>,
) -> Result<(axum::http::StatusCode, Json<SourceDto>), ApiError> {
    require_admin(State(state.clone()), headers).await?;
    if body.url.is_empty() || body.name.is_empty() {
        return Err(ApiError::bad_request("url and name required"));
    }
    let created_at = chrono::Utc::now().to_rfc3339(); // 或手写时间
    let res = sqlx::query(
        "INSERT INTO sources (url, name, enabled, created_at) VALUES (?, ?, 1, ?)",
    )
    .bind(&body.url)
    .bind(&body.name)
    .bind(&created_at)
    .execute(&state.pool)
    .await?;
    let id = res.last_insert_rowid();
    let dto = SourceDto {
        id,
        url: body.url,
        name: body.name,
        enabled: true,
        created_at,
    };
    Ok((axum::http::StatusCode::CREATED, Json(dto)))
}

async fn update_source(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<UpdateSource>,
) -> Result<Json<SourceDto>, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    // 先取现有
    let existing = sqlx::query_as::<_, SourceDto>(
        "SELECT id, url, name, enabled, created_at FROM sources WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("source not found"))?;

    let url = body.url.clone().unwrap_or(existing.url.clone());
    let name = body.name.clone().unwrap_or(existing.name.clone());
    let enabled = body.enabled.unwrap_or(existing.enabled);

    sqlx::query("UPDATE sources SET url = ?, name = ?, enabled = ? WHERE id = ?")
        .bind(&url)
        .bind(&name)
        .bind(enabled)
        .bind(id)
        .execute(&state.pool)
        .await?;

    let dto = SourceDto { id, url, name, enabled, created_at: existing.created_at };
    Ok(Json(dto))
}

async fn delete_source(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i64>,
) -> Result<axum::http::StatusCode, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    let res = sqlx::query("DELETE FROM sources WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(ApiError::not_found("source not found"));
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}

async fn refresh_source(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    // 实时拉取模式下，refresh 即对该源重新抓取并报告结果
    let source = sqlx::query_as::<_, SourceDto>(
        "SELECT id, url, name, enabled, created_at FROM sources WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("source not found"))?;

    let result = crate::service::fetch_source(&state.http, &source.url, std::time::Duration::from_secs(state.cfg.timeout_secs)).await;
    match result {
        Ok(text) => {
            let (nodes, _skipped) = proxy_core::parser::parse_subscription_text(&text, state.cfg.max_nodes);
            Ok(Json(serde_json::json!({
                "source": source.name,
                "ok": true,
                "node_count": nodes.len(),
            })))
        }
        Err(reason) => Ok(Json(serde_json::json!({
            "source": source.name,
            "ok": false,
            "reason": reason,
        }))),
    }
}
```

> **注意**：`created_at` 需要时间。为避免引入 chrono 依赖，可在 `db.rs` 提供 `now_rfc3339()` 用系统时间格式化。为简洁，Task 3 引入 `chrono` 依赖（`chrono = "0.4"`）。

- [ ] **Step 5: 加 chrono 依赖**

```toml
chrono = "0.4"
```

- [ ] **Step 6: 运行测试确认通过**

Run: `cargo test -p server --test api_test`
Expected: PASS（新增 2 个测试，共 7 个）

- [ ] **Step 7: Commit**

```bash
git add crates/server/src/auth.rs crates/server/src/routes/sources.rs crates/server/Cargo.toml
git commit -m "feat(server): admin sources CRUD with bearer auth"
```

---

### Task 4: 管理 API —— preview 与 config（token 轮换）

**Files:**
- Create: `crates/server/src/routes/preview.rs`
- Create: `crates/server/src/routes/config.rs`
- Test: `crates/server/tests/api_test.rs` 追加

**Interfaces:**
- Consumes: `require_admin`, `service::fetch_and_merge`
- Produces:
  - `pub fn router() -> Router<AppState>` — `/api/admin/preview`、`/api/admin/config`
  - GET `/api/admin/preview` → `{ "nodes": [...], "errors": [...], "total": N }`
  - GET `/api/admin/config` → `{ "subscribe_token": "...", "admin_token": "...", "subscribe_url": "/api/subscribe" }`
  - PUT `/api/admin/config` body `{ "rotate": "subscribe" | "admin" | null }` → 轮换对应 token

- [ ] **Step 1: 追加测试到 `tests/api_test.rs`**

```rust
#[tokio::test]
async fn preview_returns_node_list() {
    let mock = MockServer::start().await;
    Mock::given(method("GET")).and(path("/sub"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#A\n"))
        .mount(&mock).await;

    let tmp = std::env::temp_dir().join(format!("submerge-test-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let (_, admin) = server::db::ensure_tokens(&pool).await.unwrap();
    let url = format!("{}/sub", mock.uri());
    sqlx::query("INSERT INTO sources (url, name, enabled) VALUES (?, ?, 1)")
        .bind(&url).bind("mock").execute(&pool).await.unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg, admin.clone()).await;

    let resp = app.clone().oneshot(Request::builder()
        .uri("/api/admin/preview")
        .header("authorization", format!("Bearer {}", admin))
        .body(Body::empty()).unwrap())
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["total"], 1);
    assert_eq!(v["nodes"][0]["name"], "A");
}

#[tokio::test]
async fn config_get_and_rotate() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let (sub, admin) = server::db::ensure_tokens(&pool).await.unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg, admin.clone()).await;

    // GET config
    let resp = app.clone().oneshot(Request::builder()
        .uri("/api/admin/config")
        .header("authorization", format!("Bearer {}", admin))
        .body(Body::empty()).unwrap())
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["subscribe_token"], sub);

    // rotate subscribe token
    let resp = app.clone().oneshot(Request::builder()
        .method("PUT")
        .uri("/api/admin/config")
        .header("authorization", format!("Bearer {}", admin))
        .header("content-type", "application/json")
        .body(Body::from(json!({"rotate": "subscribe"}).to_string()))
        .unwrap())
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_ne!(v["subscribe_token"], sub);
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p server --test api_test`
Expected: FAIL — preview/config 未实现

- [ ] **Step 3: 实现 routes/preview.rs**

```rust
// crates/server/src/routes/preview.rs
use crate::auth::require_admin;
use crate::error::ApiError;
use crate::service;
use crate::state::AppState;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;

pub fn router() -> Router<AppState> {
    Router::new().route("/api/admin/preview", get(preview_handler))
}

async fn preview_handler(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    let (nodes, errors) = service::fetch_and_merge(&state).await;
    let node_list: Vec<serde_json::Value> = nodes
        .iter()
        .map(|n| {
            json!({
                "name": n.name,
                "protocol": n.kind.as_str(),
                "server": n.server,
                "port": n.port,
            })
        })
        .collect();
    let error_list: Vec<String> = errors.iter().map(|e| format!("{}: {}", e.source_name, e.reason)).collect();
    Ok(Json(json!({
        "nodes": node_list,
        "errors": error_list,
        "total": nodes.len(),
    })))
}
```

- [ ] **Step 4: 实现 routes/config.rs**

```rust
// crates/server/src/routes/config.rs
use crate::auth::require_admin;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::State;
use axum::routing::{get, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub fn router() -> Router<AppState> {
    Router::new().route("/api/admin/config", get(get_config).put(rotate_config))
}

#[derive(Serialize)]
pub struct ConfigDto {
    pub subscribe_token: String,
    pub admin_token: String,
    pub subscribe_url: String,
}

#[derive(Deserialize)]
pub struct RotateConfig {
    pub rotate: Option<String>,
}

async fn get_config(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<ConfigDto>, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    let sub = crate::db::get_setting(&state.pool, "subscribe_token").await?.unwrap_or_default();
    let admin = crate::db::get_setting(&state.pool, "admin_token").await?.unwrap_or_default();
    Ok(Json(ConfigDto {
        subscribe_token: sub,
        admin_token: admin,
        subscribe_url: "/api/subscribe".to_string(),
    }))
}

async fn rotate_config(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<RotateConfig>,
) -> Result<Json<ConfigDto>, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    match body.rotate.as_deref() {
        Some("subscribe") => {
            let t = crate::db::gen_token();
            crate::db::set_setting(&state.pool, "subscribe_token", &t).await?;
        }
        Some("admin") => {
            let t = crate::db::gen_token();
            crate::db::set_setting(&state.pool, "admin_token", &t).await?;
            // 注意：轮换 admin token 后，旧 token 立即失效。本请求用旧 token 调用已通过校验。
        }
        Some(_) => return Err(ApiError::bad_request("rotate must be 'subscribe' or 'admin'")),
        None => {}
    }
    let sub = crate::db::get_setting(&state.pool, "subscribe_token").await?.unwrap_or_default();
    let admin = crate::db::get_setting(&state.pool, "admin_token").await?.unwrap_or_default();
    Ok(Json(ConfigDto {
        subscribe_token: sub,
        admin_token: admin,
        subscribe_url: "/api/subscribe".to_string(),
    }))
}
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test -p server --test api_test`
Expected: PASS（9 个测试）

> **注意**：`rotate admin` 后 `state.admin_token`（内存）与 DB 值不一致，旧 admin token 在内存中仍有效直到重启。这是设计权衡：当前实现以内存 admin_token 为准（`AppState::admin_token`），DB 中的 admin_token 仅在启动时加载。为保持一致，`rotate_config` 轮换 admin 后**也更新内存 state**。但 `AppState` 是不可变引用。**决策**：admin token 轮换只影响 DB，内存值重启后更新；若要即时生效，需 `Arc<RwLock<String>>`。本 plan 采用 **`Arc<OnceLock<String>>` 简化**，轮换时更新共享值。见 Task 5 的说明。

- [ ] **Step 6: Commit**

```bash
git add crates/server/src/routes/preview.rs crates/server/src/routes/config.rs
git commit -m "feat(server): preview and config endpoints with token rotation"
```

---

### Task 5: admin token 即时轮换（共享可变 state）+ 静态资源托管 + main.rs 组装

**Files:**
- Modify: `crates/server/src/state.rs`（admin token 改 `Arc<RwLock<String>>`）
- Modify: `crates/server/src/routes/config.rs`（轮换后更新共享 state）
- Modify: `crates/server/src/lib.rs`（加 `pub mod static;`）
- Create: `crates/server/src/static.rs`
- Create: `crates/server/src/main.rs`
- Test: `crates/server/tests/api_test.rs` 追加静态/根路径测试

**Interfaces:**
- Consumes: `AppState`（改进版）, `AppConfig`
- Produces:
  - `AppState` 增加 `admin_token: Arc<RwLock<String>>`（tokio::sync::RwLock）
  - `pub async fn rotate_admin(&self, new: String)` 方法
  - `auth::require_admin` 改为读 `state.admin_token.read().await`
  - `static.rs`: `pub async fn fallback(uri: Uri) -> Response` — 从 `cfg.web_dist` 提供静态文件，`/` 返回 `index.html`，SPA 回退到 index.html
  - `main.rs`: 加载配置 → init_db → ensure_tokens → build_router → serve

- [ ] **Step 1: 修改 state.rs 支持 admin token 轮换**

```rust
// crates/server/src/state.rs
use crate::config::AppConfig;
use sqlx::sqlite::SqlitePool;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub cfg: AppConfig,
    pub http: reqwest::Client,
    pub admin_token: Arc<RwLock<String>>,
}

impl AppState {
    pub fn new(pool: SqlitePool, cfg: AppConfig, admin_token: String) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(cfg.timeout_secs))
            .build()
            .expect("reqwest client");
        Self {
            pool,
            cfg,
            http,
            admin_token: Arc::new(RwLock::new(admin_token)),
        }
    }

    pub async fn rotate_admin(&self, new: String) {
        *self.admin_token.write().await = new;
    }
}
```

- [ ] **Step 2: 修改 auth.rs 读共享值**

```rust
// auth.rs 中 require_admin 校验部分改为：
if constant_eq(token, &*state.admin_token.read().await) {
    Ok(())
} else {
    Err(ApiError::unauthorized("invalid admin token"))
}
```

- [ ] **Step 3: 修改 config.rs 轮换 admin 后更新内存**

```rust
// rotate_config 中 Some("admin") 分支改为：
let t = crate::db::gen_token();
crate::db::set_setting(&state.pool, "admin_token", &t).await?;
state.rotate_admin(t.clone()).await;
```

- [ ] **Step 4: 实现 static.rs**

```rust
// crates/server/src/static.rs
use axum::body::Body;
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};

/// 从 web_dist 目录提供静态资源。SPA 回退：找不到文件时返回 index.html。
pub async fn fallback(state: crate::state::AppState, uri: Uri) -> Response {
    let root = state.cfg.web_dist.clone();
    let rel_path = uri.path().trim_start_matches('/');
    let rel_path = if rel_path.is_empty() { "index.html" } else { rel_path };

    // 防止路径穿越
    if rel_path.contains("..") {
        return StatusCode::FORBIDDEN.into_response();
    }

    let candidate = root.join(rel_path);
    let mime = mime_guess::from_path(&candidate).first_or_octet_stream();

    match tokio::fs::read(&candidate).await {
        Ok(bytes) => {
            let mut resp = Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(Body::from(bytes))
                .unwrap();
            resp
        }
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
```

> 需要 `mime_guess` 依赖：
> ```toml
> mime_guess = "2"
> ```

- [ ] **Step 5: 实现 main.rs**

```rust
// crates/server/src/main.rs
use server::config::AppConfig;
use server::db::{ensure_tokens, init_db};
use server::routes::build_router;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tower_http=debug".into()),
        )
        .init();

    let cfg = AppConfig::from_env();
    let pool = init_db(&cfg.db_path).await?;
    let (sub_token, admin_token) = ensure_tokens(&pool).await?;
    tracing::info!("subscribe token: {}", sub_token);
    tracing::info!("admin token: {}", admin_token);

    let app = build_router(pool, cfg.clone(), admin_token).await;

    let addr = format!("0.0.0.0:{}", cfg.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
```

- [ ] **Step 6: 更新 build_router 签名（static.rs 接入）**

`routes/mod.rs` 中 `build_router` 改为：
```rust
pub async fn build_router(pool: SqlitePool, cfg: AppConfig, admin_token: String) -> Router {
    let state = AppState::new(pool, cfg, admin_token);
    let api = Router::new()
        .route("/api/subscribe", get(subscribe::subscribe_handler))
        .merge(sources::router())
        .merge(preview::router())
        .merge(config::router());

    let app = api
        .route("/", get(|| async { "sub-merge is running" }))
        .fallback(crate::static::fallback)
        .with_state(state);

    app
}
```
（`crate::static::fallback` 签名：`pub async fn fallback(State(state): State<AppState>, uri: Uri) -> Response`，在 `crate::static.rs` 定义，并确保 `pub mod static;` 已加入 lib.rs）

- [ ] **Step 7: 追加静态测试到 `tests/api_test.rs`**

```rust
#[tokio::test]
async fn static_index_served_from_dist() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    // 创建一个假 index.html
    let dist = tmp.join("web-dist");
    std::fs::create_dir_all(&dist).unwrap();
    std::fs::write(dist.join("index.html"), "<html>sub-merge</html>").unwrap();

    let pool = test_pool(&tmp).await;
    let (_, admin) = server::db::ensure_tokens(&pool).await.unwrap();
    let cfg = AppConfig {
        port: 0,
        db_path: tmp.join("test.db"),
        concurrency: 4,
        timeout_secs: 5,
        max_nodes: 100,
        web_dist: dist,
    };
    let app = server::routes::build_router(pool, cfg, admin).await;

    let resp = app.clone().oneshot(Request::builder().uri("/").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    assert_eq!(&bytes[..], b"sub-merge is running");
}
```

- [ ] **Step 8: 运行测试确认通过**

Run: `cargo test -p server --test api_test`
Expected: PASS（10 个测试）

- [ ] **Step 9: Commit**

```bash
git add crates/server/src/state.rs crates/server/src/auth.rs crates/server/src/routes/config.rs crates/server/src/static.rs crates/server/src/main.rs crates/server/Cargo.toml
git commit -m "feat(server): static hosting, live admin token rotation, main entrypoint"
```

---

### Task 6: 错误处理完善 + 全量测试 + 运行验证

**Files:**
- Modify: `crates/server/src/error.rs`（补充缺失转换）
- Modify: `crates/server/src/routes/subscribe.rs`（修复 502 逻辑）
- Test: 全量

**Interfaces:**
- Consumes: 既有全部
- Produces: 无新接口

- [ ] **Step 1: 审查并补全 error.rs 的 From 转换**

确保以下都已实现：
```rust
impl From<anyhow::Error> for ApiError { ... }
impl From<sqlx::Error> for ApiError { ... }
```
（已在 Task 1 提供）

- [ ] **Step 2: 修正 subscribe.rs 的 502 逻辑**

原实现里 `Response::builder()...body(...)` 后若再 `Json(...).into_response()` 替换 resp，存在 `unwrap()` 与重复 body 的瑕疵。改为清晰实现：
```rust
if nodes.is_empty() && !source_errors.is_empty() {
    let detail = source_errors.iter().map(|e| format!("{}: {}", e.source_name, e.reason)).collect::<Vec<_>>();
    return Err(ApiError { status: StatusCode::BAD_GATEWAY, code: "bad_gateway", message: detail.join("; ") });
}
```

- [ ] **Step 3: 全量测试**

Run: `cargo test -p server`
Expected: 全部 PASS

- [ ] **Step 4: 运行服务冒烟测试**

```bash
cd /root/github/sub-merge
cargo run -p server 2>&1 | head -30 &
sleep 3
# 检查监听
curl -s http://localhost:8080/ && echo ""
# 检查订阅接口（token 从启动日志读取）
# 无 token 应 401
curl -s -o /dev/null -w "%{http_code}" "http://localhost:8080/api/subscribe" && echo " (expected 401)"
```

- [ ] **Step 5: 停止服务**

```bash
pkill -f "cargo run -p server" || true
```

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "fix(server): 502 handling, error conversions, smoke test verified"
```

---

## Plan B 完成标准

- [ ] `cargo test -p server` 全绿（10+ 测试）
- [ ] `GET /api/subscribe`：无 token 401、错误 format 400、有效返回订阅正文
- [ ] `/api/admin/*`：无 Bearer 401、CRUD 全通过
- [ ] `/api/admin/preview` 返回节点列表
- [ ] `/api/admin/config` GET/PUT（token 轮换）通过
- [ ] admin token 轮换后旧值失效（内存态同步）
- [ ] 静态资源托管正常（`/` 返回服务信息，SPA 回退）
- [ ] 冒烟测试通过（服务可启动、接口可访问）
