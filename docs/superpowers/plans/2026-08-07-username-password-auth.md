# 管理端认证改造（用户名+密码登录）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把管理端认证从 Bearer admin token 改为用户名+密码登录：首次运行引导创建管理员，登录发随机会话 token，密码 argon2 哈希存储。

**Architecture:** 后端新增 `users`/`sessions` 两张表（密码 argon2 PHC 存储、会话 token 存库前 sha256），新增 4 个认证端点（setup-status/setup/login/logout），`require_admin` 从「恒定时间比较内存 token」改为「sha256(token) 查 sessions 表」，config 路由改为返回用户名/修改密码。前端登录页双模式（无管理员→创建表单，有→登录表单），localStorage 存会话 token 沿用 Bearer 请求头。

**Tech Stack:** argon2 0.5（RustCrypto，纯 Rust）+ sha2 0.10；axum 0.8；sqlx 0.9 SQLite；dioxus 0.8.0-alpha.1（前端）。

## Global Constraints

- 每次代码修改后必须按序通过：`cargo upgrade -i` → `cargo fmt --all` → `cargo clippy --workspace` → `cargo test --workspace`（CLAUDE.md 强制）
- Rust edition 2024（workspace 继承；web crate 字面 `edition = "2024"`），rust-version 1.97
- web crate 独立于 workspace（不进 members），仅由 `dx build --web --release --debug-symbols false` 构建
- 用户名规则：trim 后 `[A-Za-z0-9-_]{1,64}`；密码（不 trim）≥8 字符
- 会话 token：32 字节随机 hex，存库前 sha256；会话永久有效；修改密码后**全部会话（含当前）立即失效**
- 无鉴权端点注册为路由、handler 内显式调用 `require_admin`（不引入中间件层）
- 前端请求头逻辑（`Authorization: Bearer`）不变；localStorage key 改为 `submerge_admin_session`
- `SUB_MERGE_ADMIN_TOKEN` 环境变量删除，不留替代

---

### Task 1: 数据层 — users/sessions 表 + 密码/会话函数族

**Files:**
- Modify: `crates/server/Cargo.toml`（加 argon2、sha2 依赖）
- Modify: `crates/server/src/db.rs`
- Test: `crates/server/tests/api_test.rs`

**Interfaces:**
- Consumes: 无（新功能）
- Produces（后续任务依赖的精确签名，均在 `db.rs`）：
  - `pub async fn users_empty(pool: &SqlitePool) -> Result<bool>`
  - `pub async fn create_user(pool: &SqlitePool, username: &str, password: &str) -> Result<()>`（argon2 hash 后插入，UNIQUE 冲突返回 Err）
  - `pub async fn verify_user(pool: &SqlitePool, username: &str, password: &str) -> Result<bool>`（用户不存在或密码错均返回 false）
  - `pub async fn update_password(pool: &SqlitePool, username: &str, new_password: &str) -> Result<()>`
  - `pub async fn get_username(pool: &SqlitePool) -> Result<Option<String>>`（单用户，取唯一一行）
  - `pub async fn create_session(pool: &SqlitePool) -> Result<String>`（生成 32B hex，存 sha256，返回明文 token）
  - `pub async fn validate_session(pool: &SqlitePool, token: &str) -> Result<bool>`
  - `pub async fn delete_session(pool: &SqlitePool, token: &str) -> Result<()>`
  - `pub async fn delete_all_sessions(pool: &SqlitePool) -> Result<()>`

- [ ] **Step 1: 加依赖**

`crates/server/Cargo.toml` 的 `[dependencies]` 追加：

```toml
argon2 = "0.5"
sha2 = "0.10"
```

- [ ] **Step 2: 建表（db.rs `init_db` 末尾追加）**

```sql
CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS sessions (
    token_hash TEXT PRIMARY KEY,
    created_at TEXT NOT NULL
);
```

- [ ] **Step 3: 写失败测试（api_test.rs 追加）**

```rust
#[tokio::test]
async fn user_and_session_db_functions() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-auth-db", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;

    // 初始无用户
    assert!(server::db::users_empty(&pool).await.unwrap());

    // 创建 + 验证
    server::db::create_user(&pool, "admin", "correct-password").await.unwrap();
    assert!(!server::db::users_empty(&pool).await.unwrap());
    assert!(server::db::verify_user(&pool, "admin", "correct-password").await.unwrap());
    assert!(!server::db::verify_user(&pool, "admin", "wrong-password").await.unwrap());
    assert!(!server::db::verify_user(&pool, "nobody", "correct-password").await.unwrap());
    assert_eq!(server::db::get_username(&pool).await.unwrap().as_deref(), Some("admin"));

    // 重名 → Err
    assert!(server::db::create_user(&pool, "admin", "x").await.is_err());

    // 会话生命周期
    let t1 = server::db::create_session(&pool).await.unwrap();
    let t2 = server::db::create_session(&pool).await.unwrap();
    assert_ne!(t1, t2);
    assert_eq!(t1.len(), 64); // 32 bytes hex
    assert!(server::db::validate_session(&pool, &t1).await.unwrap());
    assert!(!server::db::validate_session(&pool, "f".repeat(64).as_str()).await.unwrap());
    server::db::delete_session(&pool, &t1).await.unwrap();
    assert!(!server::db::validate_session(&pool, &t1).await.unwrap());
    server::db::delete_all_sessions(&pool).await.unwrap();
    assert!(!server::db::validate_session(&pool, &t2).await.unwrap());

    // 改密码后旧密码失效、新密码生效
    server::db::update_password(&pool, "admin", "new-password").await.unwrap();
    assert!(!server::db::verify_user(&pool, "admin", "correct-password").await.unwrap());
    assert!(server::db::verify_user(&pool, "admin", "new-password").await.unwrap());
}
```

- [ ] **Step 4: 跑测试确认失败**

Run: `cargo test --test api_test user_and_session_db_functions`
Expected: 编译错误（`server::db::users_empty` 等函数不存在）

- [ ] **Step 5: 实现函数族（db.rs）**

依赖导入：

```rust
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng};
use argon2::{Argon2, password_hash::Error as ArgonError};
use sha2::{Digest, Sha256};
```

实现（置于 `gen_token` 之后）：

```rust
fn sha256_hex(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    const_hex::encode(h.finalize())
}

pub async fn users_empty(pool: &SqlitePool) -> Result<bool> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(pool).await?;
    Ok(n == 0)
}

fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("argon2 hash failed: {e}"))?
        .to_string();
    Ok(hash)
}

fn verify_password_hash(password: &str, stored: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(stored) else { return false };
    Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok()
}

pub async fn create_user(pool: &SqlitePool, username: &str, password: &str) -> Result<()> {
    let hash = hash_password(password)?;
    let created_at = chrono::Utc::now().to_rfc3339();
    sqlx::query("INSERT INTO users (username, password_hash, created_at) VALUES (?, ?, ?)")
        .bind(username).bind(&hash).bind(created_at)
        .execute(pool).await?;
    Ok(())
}

pub async fn verify_user(pool: &SqlitePool, username: &str, password: &str) -> Result<bool> {
    let row = sqlx::query("SELECT password_hash FROM users WHERE username = ?")
        .bind(username).fetch_optional(pool).await?;
    let Some(row) = row else { return Ok(false) };
    let stored: String = row.get(0);
    Ok(verify_password_hash(password, &stored))
}

pub async fn update_password(pool: &SqlitePool, username: &str, new_password: &str) -> Result<()> {
    let hash = hash_password(new_password)?;
    sqlx::query("UPDATE users SET password_hash = ? WHERE username = ?")
        .bind(hash).bind(username).execute(pool).await?;
    Ok(())
}

pub async fn get_username(pool: &SqlitePool) -> Result<Option<String>> {
    let row = sqlx::query("SELECT username FROM users LIMIT 1").fetch_optional(pool).await?;
    Ok(row.map(|r| r.get::<String, _>(0)))
}

pub async fn create_session(pool: &SqlitePool) -> Result<String> {
    let token = gen_token();
    let created_at = chrono::Utc::now().to_rfc3339();
    sqlx::query("INSERT INTO sessions (token_hash, created_at) VALUES (?, ?)")
        .bind(sha256_hex(&token)).bind(created_at)
        .execute(pool).await?;
    Ok(token)
}

pub async fn validate_session(pool: &SqlitePool, token: &str) -> Result<bool> {
    let row = sqlx::query("SELECT 1 FROM sessions WHERE token_hash = ?")
        .bind(sha256_hex(token)).fetch_optional(pool).await?;
    Ok(row.is_some())
}

pub async fn delete_session(pool: &SqlitePool, token: &str) -> Result<()> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
        .bind(sha256_hex(token)).execute(pool).await?;
    Ok(())
}

pub async fn delete_all_sessions(pool: &SqlitePool) -> Result<()> {
    sqlx::query("DELETE FROM sessions").execute(pool).await?;
    Ok(())
}
```

注意：`row.get(0)` 需要 `use sqlx::Row;`（db.rs 已有）。

- [ ] **Step 6: 跑测试确认通过**

Run: `cargo test --test api_test user_and_session_db_functions`
Expected: PASS

- [ ] **Step 7: 全量门禁 + commit**

Run: `cargo fmt --all && cargo clippy --workspace && cargo test --workspace`
Expected: 全通过（旧 token 功能未动，现有测试不受影响）

```bash
git add crates/server/Cargo.toml crates/server/src/db.rs crates/server/tests/api_test.rs
git commit -m "feat(db): users/sessions 表 + argon2 密码哈希 + 会话函数族"
```

---

### Task 2: 认证路由 — setup-status / setup / login / logout

**Files:**
- Create: `crates/server/src/routes/auth.rs`
- Modify: `crates/server/src/routes/mod.rs`（挂载路由）
- Modify: `crates/server/src/error.rs`（`ApiError::conflict`）
- Test: `crates/server/tests/api_test.rs`

**Interfaces:**
- Consumes: Task 1 的 db 函数族（`users_empty`/`create_user`/`verify_user`/`create_session`/`delete_session`）；`build_router(pool, cfg, admin)` 现状签名（Task 3 才改）
- Produces: 四个端点 + `extract_bearer(&HeaderMap) -> Option<&str>`（auth.rs 内，Task 3 复用）

- [ ] **Step 1: error.rs 加 409**

```rust
pub fn conflict(msg: impl Into<String>) -> Self {
    Self {
        status: StatusCode::CONFLICT,
        code: "conflict",
        message: msg.into(),
    }
}
```

- [ ] **Step 2: 写失败测试（api_test.rs 追加）**

```rust
fn valid_setup(name: &str) -> String {
    json!({"username": name, "password": "pass-12345", "password_confirm": "pass-12345"}).to_string()
}

async fn http(app: &axum::Router, method: &str, uri: &str, body: Option<String>, token: Option<&str>)
    -> (axum::http::StatusCode, serde_json::Value) {
    let mut b = axum::http::Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        b = b.header("authorization", format!("Bearer {t}"));
    }
    let req = match body {
        Some(s) => b.header("content-type", "application/json").body(axum::body::Body::from(s)).unwrap(),
        None => b.body(axum::body::Body::empty()).unwrap(),
    };
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let v = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, v)
}

#[tokio::test]
async fn setup_creates_admin_once() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-setup", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg, "admin-token".to_string()).await;

    // setup-status：未创建 → needs_setup true
    let (s, v) = http(&app, "GET", "/admin/setup-status", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["needs_setup"], serde_json::Value::Bool(true));

    // 创建成功 → 200
    let (s, v) = http(&app, "POST", "/admin/setup", Some(valid_setup("admin")), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["username"], "admin");

    // 已创建：setup-status false + setup 409 锁定
    let (s, v) = http(&app, "GET", "/admin/setup-status", None, None).await;
    assert_eq!(v["needs_setup"], serde_json::Value::Bool(false));
    let (s, _) = http(&app, "POST", "/admin/setup", Some(valid_setup("admin2")), None).await;
    assert_eq!(s, StatusCode::CONFLICT);
}

#[tokio::test]
async fn setup_validates_fields() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-setup-val", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg, "admin-token".to_string()).await;

    // 密码过短 → 400
    let (s, _) = http(&app, "POST", "/admin/setup",
        Some(json!({"username": "a", "password": "short", "password_confirm": "short"}).to_string()), None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // 两次不一致 → 400
    let (s, _) = http(&app, "POST", "/admin/setup",
        Some(json!({"username": "a", "password": "pass-12345", "password_confirm": "different"}).to_string()), None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // 非法用户名 → 400
    let (s, _) = http(&app, "POST", "/admin/setup",
        Some(json!({"username": "bad name!", "password": "pass-12345", "password_confirm": "pass-12345"}).to_string()), None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn login_and_logout_flow() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-login", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg, "admin-token".to_string()).await;

    http(&app, "POST", "/admin/setup", Some(valid_setup("admin")), None).await;

    // 正确凭证 → 200 + token（64 hex）
    let (s, v) = http(&app, "POST", "/admin/login",
        Some(json!({"username": "admin", "password": "pass-12345"}).to_string()), None).await;
    assert_eq!(s, StatusCode::OK);
    let token = v["token"].as_str().unwrap().to_string();
    assert_eq!(token.len(), 64);

    // 错误密码 / 不存在用户 → 统一 401
    for body in [
        json!({"username": "admin", "password": "wrong"}).to_string(),
        json!({"username": "nobody", "password": "pass-12345"}).to_string(),
    ] {
        let (s, v) = http(&app, "POST", "/admin/login", Some(body), None).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        assert_eq!(v["error"]["code"], "unauthorized");
    }

    // logout 后 token 失效（此时 require_admin 还是旧 token 实现，用 GET /admin/setup-status 无法验证
    // 会话删除——改由 Task 3 的鉴权切换后验证；此处只断言 logout 返回 204 且无鉴权问题）
    let (s, _) = http(&app, "POST", "/admin/logout", None, Some(&token)).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
}
```

- [ ] **Step 3: 跑测试确认失败**

Run: `cargo test --test api_test setup_creates_admin_once`
Expected: 404（路由未注册）

- [ ] **Step 4: 实现 auth.rs 路由**

```rust
// crates/server/src/routes/auth.rs
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::http::header::AUTHORIZATION;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct SetupRequest { pub username: String, pub password: String, pub password_confirm: String }

#[derive(Deserialize)]
pub struct LoginRequest { pub username: String, pub password: String }

#[derive(Serialize)]
pub struct LoginResp { pub token: String }

#[derive(Serialize)]
pub struct SetupStatusResp { pub needs_setup: bool }

/// 提取 Bearer token（require_admin 与 logout 复用）
pub fn extract_bearer(headers: &HeaderMap) -> Option<&str> {
    let auth = headers.get(AUTHORIZATION)?.to_str().ok()?;
    auth.strip_prefix("Bearer ")
}

fn valid_username(s: &str) -> bool {
    let t = s.trim();
    !t.is_empty() && t.len() <= 64
        && t.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/setup-status", get(setup_status))
        .route("/admin/setup", post(setup))
        .route("/admin/login", post(login))
        .route("/admin/logout", post(logout))
}

async fn setup_status(State(state): State<AppState>) -> Result<Json<SetupStatusResp>, ApiError> {
    let needs_setup = crate::db::users_empty(&state.pool).await?;
    Ok(Json(SetupStatusResp { needs_setup }))
}

async fn setup(
    State(state): State<AppState>,
    body: Result<Json<SetupRequest>, JsonRejection>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let Json(b) = body.map_err(ApiError::from)?;
    if !crate::db::users_empty(&state.pool).await? {
        return Err(ApiError::conflict("admin user already exists"));
    }
    let username = b.username.trim().to_string();
    if !valid_username(&username) {
        return Err(ApiError::bad_request("username must match [A-Za-z0-9-_] (1-64 chars)"));
    }
    if b.password.len() < 8 {
        return Err(ApiError::bad_request("password must be at least 8 characters"));
    }
    if b.password != b.password_confirm {
        return Err(ApiError::bad_request("passwords do not match"));
    }
    crate::db::create_user(&state.pool, &username, &b.password).await?;
    Ok(Json(serde_json::json!({ "username": username })))
}

async fn login(
    State(state): State<AppState>,
    body: Result<Json<LoginRequest>, JsonRejection>,
) -> Result<Json<LoginResp>, ApiError> {
    let Json(b) = body.map_err(ApiError::from)?;
    if !crate::db::verify_user(&state.pool, b.username.trim(), &b.password).await? {
        return Err(ApiError::unauthorized("invalid username or password"));
    }
    let token = crate::db::create_session(&state.pool).await?;
    Ok(Json(LoginResp { token }))
}

async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<axum::http::StatusCode, ApiError> {
    let Some(token) = extract_bearer(&headers) else {
        return Err(ApiError::unauthorized("missing authorization header"));
    };
    // logout 删除会话即注销；token 不存在也返回 204（幂等）
    crate::db::delete_session(&state.pool, token).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}
```

- [ ] **Step 5: 挂载路由（routes/mod.rs）**

```rust
pub mod auth;
// ...
let api = Router::new()
    .route("/subscribe/{name}", get(subscribe::subscribe_handler))
    .merge(auth::router())
    .merge(combineds::router())
    // ...
```

- [ ] **Step 6: 跑测试确认通过**

Run: `cargo test --test api_test setup_creates_admin_once setup_validates_fields login_and_logout_flow`
Expected: 全 PASS

- [ ] **Step 7: 全量门禁 + commit**

Run: `cargo fmt --all && cargo clippy --workspace && cargo test --workspace`
Expected: 全通过（Task 3 前 build_router 签名未变，现有测试不受影响）

```bash
git add crates/server/src/error.rs crates/server/src/routes/auth.rs crates/server/src/routes/mod.rs crates/server/tests/api_test.rs
git commit -m "feat(routes): 新增 setup-status/setup/login/logout 认证端点"
```

---

### Task 3: 鉴权链路切换（token → session）

**Files:**
- Modify: `crates/server/src/auth.rs`（require_admin 改查 sessions）
- Modify: `crates/server/src/state.rs`（删 admin_token 字段 + rotate_admin）
- Modify: `crates/server/src/routes/mod.rs`（build_router 签名去 admin_token）
- Modify: `crates/server/src/main.rs`（删 ensure_tokens/首启打印）
- Modify: `crates/server/src/db.rs`（删 ensure_tokens/ensure_tokens_with/tokens_initialized）
- Modify: `crates/server/tests/api_test.rs`（测试夹具 + 删 3 个 token 测试）
- Modify: `crates/server/tests/final_fixes.rs`（build_router 调用点）

**Interfaces:**
- Consumes: Task 2 的 `extract_bearer`、四个端点；Task 1 的 `validate_session`
- Produces: `build_router(pool: SqlitePool, cfg: AppConfig) -> Router`（新签名，无 admin_token 参数）

- [ ] **Step 1: auth.rs 改 require_admin**

```rust
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::State;
use axum::http::HeaderMap;

pub fn extract_bearer(headers: &HeaderMap) -> Option<&str> { /* 从 routes/auth.rs 移到此处 */ }

/// 校验 Bearer 会话 token：sha256(token) 查 sessions 表。返回 Ok(()) 或 401。
pub async fn require_admin(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(), ApiError> {
    let Some(token) = extract_bearer(&headers) else {
        return Err(ApiError::unauthorized("missing authorization header"));
    };
    if crate::db::validate_session(&state.pool, token).await? {
        Ok(())
    } else {
        Err(ApiError::unauthorized("invalid session"))
    }
}
```

同时删除 `constant_eq` 函数。`routes/auth.rs` 删除本地 `extract_bearer` 定义，改为 `use crate::auth::extract_bearer;`（auth.rs 是 `pub mod auth`，routes 里 `use crate::auth::...` 可访问）。

- [ ] **Step 2: state.rs 删 admin_token**

```rust
pub struct AppState {
    pub pool: SqlitePool,
    pub cfg: AppConfig,
    pub http: reqwest::Client,
    pub fetch_semaphore: Arc<Semaphore>,
}

impl AppState {
    pub fn new(pool: SqlitePool, cfg: AppConfig) -> Self {
        // ...删除 admin_token: Arc::new(RwLock::new(admin_token)) 与 rotate_admin 方法
    }
}
```

（`tokio::sync::RwLock` 若不再被使用则移除该 use）

- [ ] **Step 3: build_router 签名（routes/mod.rs）**

```rust
pub async fn build_router(pool: sqlx::sqlite::SqlitePool, cfg: crate::config::AppConfig) -> Router {
    let state = AppState::new(pool, cfg);
    // ...其余不变
}
```

- [ ] **Step 4: main.rs 清理**

```rust
let cfg = AppConfig::from_env();
let pool = init_db(&cfg.db_path).await?;
let app = build_router(pool, cfg).await;
// 删除 first_init / admin_token / ensure_tokens / 首启 warn 日志 / debug 日志
```

- [ ] **Step 5: db.rs 删旧 token 函数**

删除 `tokens_initialized` / `ensure_tokens` / `ensure_tokens_with`（`gen_token` 保留——`create_session` 在用；`get_setting`/`set_setting` 保留——`config.rs` 的 rotate 仍在用，Task 4 删除）。

- [ ] **Step 6: api_test.rs 夹具改造**

顶部加 helper（置于 `test_pool` 之后）：

```rust
/// 走真实 HTTP 链路创建管理员并登录，返回会话 token
async fn setup_admin(app: &axum::Router) -> String {
    let (s, _) = http(app, "POST", "/admin/setup",
        Some(valid_setup("admin")), None).await;
    assert_eq!(s, StatusCode::OK, "setup must succeed");
    let (s, v) = http(app, "POST", "/admin/login",
        Some(json!({"username": "admin", "password": "pass-12345"}).to_string()), None).await;
    assert_eq!(s, StatusCode::OK, "login must succeed");
    v["token"].as_str().unwrap().to_string()
}
```

`valid_setup` 与 `http` helper 已在 Task 2 定义（追加到文件顶部，Task 2 若已加在测试函数下方则移到顶部）。全部既有测试按如下规则改造：

```rust
// 之前（约 15 处，模式统一）：
let pool = test_pool(&tmp).await;
let admin = server::db::ensure_tokens(&pool).await.unwrap();
let cfg = test_config(&tmp);
let app = server::routes::build_router(pool, cfg, admin.clone()).await;
// 之后：
let pool = test_pool(&tmp).await;
let cfg = test_config(&tmp);
let app = server::routes::build_router(pool, cfg).await;
let admin = setup_admin(&app).await;
```

注意顺序：`build_router` 不再接收 token，必须先 build 再 setup_admin。`auth` closure 不变（仍是 `Bearer {admin}`，此时 admin 是会话 token）。

删除这三个测试（token 概念已移除，覆盖逻辑被 Task 1 的 `user_and_session_db_functions` 与 Task 2 的新测试承接）：
- `db_creates_tables_and_tokens`
- `env_preset_tokens_used_only_on_first_init`
- `tokens_initialized_reflects_first_init`

`config_get_and_rotate` 与 `admin_token_rotation_takes_effect_live` 暂保留编译（config.rs 的 rotate 实现 Task 4 才改），`subscribe_without_token_succeeds` 等只涉及 build_router 调用的测试同步改签名。

- [ ] **Step 7: final_fixes.rs 改造**

`build_router(pool, cfg, "admin-token".to_string()).await` → `build_router(pool, cfg).await`。`refresh_source_zero_nodes_reports_ok_false` 的 `header("authorization", format!("Bearer {admin_token}"))` 需要合法会话——改为直接走 db 层：

```rust
let pool = init_db(&db_path).await.unwrap();
let cfg = AppConfig { /* ... */ };
let app = build_router(pool.clone(), cfg).await;
server::db::create_user(&pool, "admin", "pass-12345").await.unwrap();
let session = server::db::create_session(&pool).await.unwrap();
// header: format!("Bearer {session}")
```

- [ ] **Step 8: 跑全量测试**

Run: `cargo test --workspace`
Expected: 全 PASS（含 Task 2 的 `login_and_logout_flow` 中 logout 幂等断言）

- [ ] **Step 9: 全量门禁 + commit**

Run: `cargo fmt --all && cargo clippy --workspace && cargo test --workspace`

```bash
git add crates/server/src/ crates/server/tests/
git commit -m "refactor(auth): 鉴权从 Bearer admin token 切换为会话 token（sha256 查 sessions 表）"
```

---

### Task 4: config 路由改造 — 返回用户名 + 修改密码

**Files:**
- Modify: `crates/server/src/routes/config.rs`
- Modify: `crates/server/src/db.rs`（删 get_setting/set_setting——rotate 删除后无调用方）
- Test: `crates/server/tests/api_test.rs`

**Interfaces:**
- Consumes: Task 1 的 `get_username`/`update_password`/`delete_all_sessions`/`verify_user`；Task 3 的 build_router 新签名
- Produces: `GET /admin/config` → `{"username": "..."}`；`PUT /admin/config` body `{"change_password": {"old": "...", "new": "..."}}`

- [ ] **Step 1: 写失败测试（api_test.rs 重写 config 测试）**

删除 `config_get_and_rotate` 与 `admin_token_rotation_takes_effect_live`，替换为：

```rust
#[tokio::test]
async fn config_returns_username() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-cfg-user", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
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
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-chpass", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg).await;
    let admin = setup_admin(&app).await;
    // 第二台设备：再登录拿一个会话
    let (_, v) = http(&app, "POST", "/admin/login",
        Some(json!({"username": "admin", "password": "pass-12345"}).to_string()), None).await;
    let second = v["token"].as_str().unwrap().to_string();

    // 旧密码错误 → 400
    let (s, _) = http(&app, "PUT", "/admin/config",
        Some(json!({"change_password": {"old": "wrong", "new": "new-pass-678"}}).to_string()), Some(&admin)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // 新密码过短 → 400
    let (s, _) = http(&app, "PUT", "/admin/config",
        Some(json!({"change_password": {"old": "pass-12345", "new": "short"}}).to_string()), Some(&admin)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // 正确改密 → 200，返回 username
    let (s, v) = http(&app, "PUT", "/admin/config",
        Some(json!({"change_password": {"old": "pass-12345", "new": "new-pass-678"}}).to_string()), Some(&admin)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["username"], "admin");

    // 全部旧会话（含当前）立即失效 → 401
    let (s, _) = http(&app, "GET", "/admin/config", None, Some(&admin)).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = http(&app, "GET", "/admin/config", None, Some(&second)).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // 新密码可登录
    let (s, v) = http(&app, "POST", "/admin/login",
        Some(json!({"username": "admin", "password": "new-pass-678"}).to_string()), None).await;
    assert_eq!(s, StatusCode::OK);
    let new_token = v["token"].as_str().unwrap().to_string();
    let (s, _) = http(&app, "GET", "/admin/config", None, Some(&new_token)).await;
    assert_eq!(s, StatusCode::OK);
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --test api_test config_returns_username`
Expected: FAIL（GET 返回 admin_token 而非 username）

- [ ] **Step 3: 实现 config.rs**

```rust
use crate::auth::require_admin;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

pub fn router() -> Router<AppState> {
    Router::new().route("/admin/config", get(get_config).put(update_config))
}

#[derive(Serialize)]
pub struct ConfigDto { pub username: String }

#[derive(Deserialize)]
pub struct UpdateConfig {
    pub change_password: Option<ChangePassword>,
}

#[derive(Deserialize)]
pub struct ChangePassword { pub old: String, pub new: String }

async fn config_dto(state: &AppState) -> Result<ConfigDto, ApiError> {
    let username = crate::db::get_username(&state.pool)
        .await?
        .ok_or_else(|| ApiError::internal("no admin user"))?;
    Ok(ConfigDto { username })
}

async fn get_config(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<ConfigDto>, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    Ok(Json(config_dto(&state).await?))
}

async fn update_config(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: Result<Json<UpdateConfig>, JsonRejection>,
) -> Result<Json<ConfigDto>, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    let Json(b) = body.map_err(ApiError::from)?;
    if let Some(cp) = &b.change_password {
        let username = crate::db::get_username(&state.pool)
            .await?
            .ok_or_else(|| ApiError::internal("no admin user"))?;
        if !crate::db::verify_user(&state.pool, &username, &cp.old).await? {
            return Err(ApiError::bad_request("old password is incorrect"));
        }
        if cp.new.len() < 8 {
            return Err(ApiError::bad_request("password must be at least 8 characters"));
        }
        crate::db::update_password(&state.pool, &username, &cp.new).await?;
        // 修改密码后全部会话（含当前）立即失效
        crate::db::delete_all_sessions(&state.pool).await?;
    }
    Ok(Json(config_dto(&state).await?))
}
```

- [ ] **Step 4: db.rs 删 get_setting/set_setting**

删除 `get_setting` / `set_setting`（rotate 已删除，无调用方）。删除后检查 `api_test.rs` 中引用（原 `db_creates_tables_and_tokens` 已删，无残留）。

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test --test api_test config_returns_username change_password_invalidates_all_sessions`
Expected: PASS

- [ ] **Step 6: 全量门禁 + commit**

Run: `cargo fmt --all && cargo clippy --workspace && cargo test --workspace`

```bash
git add crates/server/src/routes/config.rs crates/server/src/db.rs crates/server/tests/api_test.rs
git commit -m "feat(config): 返回用户名 + 修改密码（改密后全部会话失效），删除 rotate"
```

---

### Task 5: 前端改造 — 登录页双模式 + 账号卡片 + web-core DTO

**Files:**
- Modify: `crates/web-core/src/dto.rs`（ConfigDto username + fixture 测试）
- Modify: `crates/server/web/src/components/login.rs`（双模式）
- Modify: `crates/server/web/src/components/config.rs`（账号卡片 + 改密码）
- Modify: `crates/server/web/src/main.rs`（退出登录调服务端注销）
- Test: `crates/web-core/src/dto.rs`（fixture 测试内联）

**Interfaces:**
- Consumes: 后端 Task 2/3/4 的端点与响应形状
- Produces: 前端可运行 WASM；验证 = `dx build --web --release --debug-symbols false`

- [ ] **Step 1: web-core ConfigDto 改字段（dto.rs）**

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct ConfigDto {
    pub username: String,
}
```

测试 `config_dto_parses` fixture 改为：

```rust
let j = r#"{"username":"admin"}"#;
let d: ConfigDto = serde_json::from_str(j).unwrap();
assert_eq!(d.username, "admin");
```

- [ ] **Step 2: 跑 workspace 测试确认**

Run: `cargo test -p submerge-web-core`
Expected: PASS（含新 fixture）

- [ ] **Step 3: login.rs 双模式**

在现有 Login 组件基础上改造：

```rust
// localStorage key 改为 submerge_admin_session
const STORAGE_KEY: &str = "submerge_admin_session";

pub fn read_token() -> Option<String> {
    let w = web_sys::window()?;
    let s = w.local_storage().ok().flatten()?;
    s.get_item(STORAGE_KEY).ok().flatten()
}
pub fn write_token(t: &str) {
    if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = s.set_item(STORAGE_KEY, t);
    }
}
pub fn clear_token() {
    if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = s.remove_item(STORAGE_KEY);
    }
}
```

Login 组件增加 setup 模式（关键结构，沿用项目既有信号/rsx 模式）：

```rust
#[component]
pub fn Login(on_login: EventHandler<String>) -> Element {
    let mut needs_setup = use_signal(|| None::<bool>); // None=加载中
    let mut input = use_signal(String::new);
    let mut setup_user = use_signal(String::new);
    let mut setup_pass = use_signal(String::new);
    let mut setup_pass2 = use_signal(String::new);
    let mut error = use_signal(String::new);
    let mut loading = use_signal(|| false);

    // 挂载时探测 setup 状态（无依赖数组，用 None 守卫一次性执行）
    use_future(move || async move {
        match request("GET", "/admin/setup-status", None, None).await {
            Ok(body) => match serde_json::from_str::<serde_json::Value>(&body) {
                Ok(v) => needs_setup.set(Some(v["needs_setup"].as_bool().unwrap_or(false))),
                Err(e) => error.set(format!("解析失败: {e}")),
            },
            Err(e) => error.set(format!("检查初始化状态失败: {e}")),
        }
    });
    // ...（needs_setup 为 None 时渲染 Spinner；Some(true) 渲染创建表单；Some(false) 渲染登录表单）
}
```

创建流程（与登录共用 loading/error，成功后自动登录）：

```rust
let mut do_setup = move || {
    let user = setup_user.read().clone();
    let pass = setup_pass.read().clone();
    let pass2 = setup_pass2.read().clone();
    if user.is_empty() || pass.is_empty() {
        error.set("请填写用户名与密码".into());
        return;
    }
    if pass != pass2 {
        error.set("两次输入的密码不一致".into());
        return;
    }
    loading.set(true);
    spawn(async move {
        let body = serde_json::json!({"username": user, "password": pass, "password_confirm": pass2}).to_string();
        match request("POST", "/admin/setup", Some(body), None).await {
            Ok(_) => {
                // 创建成功 → 自动登录
                let login_body = serde_json::json!({"username": user, "password": pass}).to_string();
                match request("POST", "/admin/login", Some(login_body), None).await {
                    Ok(b) => match serde_json::from_str::<serde_json::Value>(&b) {
                        Ok(v) => on_login.call(v["token"].as_str().unwrap_or_default().to_string()),
                        Err(e) => error.set(format!("解析失败: {e}")),
                    },
                    Err(e) => error.set(format!("登录失败: {e}")),
                }
            }
            Err(e) => error.set(format!("创建失败: {e}")),
        }
        loading.set(false);
    });
};
```

rsx 中 `needs_setup` 为 None 时渲染全页 Spinner（参考 App 启动校验的 login-logo Spinner 模式）。

- [ ] **Step 4: config.rs 账号卡片**

删除 Token 卡片逻辑，改为：

```rust
// 状态
let mut old_pass = use_signal(String::new);
let mut new_pass = use_signal(String::new);
let mut new_pass2 = use_signal(String::new);
let mut changing = use_signal(|| false);

// 提交改密
let mut do_change = move || {
    let old = old_pass.read().clone();
    let new_p = new_pass.read().clone();
    let new2 = new_pass2.read().clone();
    if new_p.is_empty() || old.is_empty() {
        error.set("请填写完整".into());
        return;
    }
    if new_p != new2 {
        error.set("两次输入的新密码不一致".into());
        return;
    }
    let current = token.read().clone();
    let body = serde_json::json!({"change_password": {"old": old, "new": new_p}}).to_string();
    let mut token2 = token.clone();
    let toasts = toasts.clone();
    changing.set(true);
    spawn(async move {
        match request("PUT", "/admin/config", Some(body), current.as_deref()).await {
            Ok(_) => {
                push_toast(toasts, ToastKind::Success, "密码已修改，请重新登录");
                clear_token();
                token2.set(None); // 会话已失效，回登录页
            }
            Err(e) => error.set(format!("修改失败: {e}")),
        }
        changing.set(false);
    });
};
```

卡片渲染：显示 `username`（来自 `config_state.data` 的 ConfigDto）+ 三个密码输入框 + 提交按钮。删除 `mask_token` / `show_admin` / `rotate` / `ask_rotate` / ConfirmDialog 相关代码（`submerge_web_core::fmt::mask_token` import 一并删）。

- [ ] **Step 5: main.rs 退出登录调服务端注销**

```rust
button { class: "btn btn-ghost btn-sm", onclick: move |_| {
    let t = token.read().clone();
    let mut token = token.clone();
    spawn(async move {
        // 服务端注销会话（失败也照清本地，本地退出兜底）
        if let Some(t) = t {
            let _ = request("POST", "/admin/logout", None, Some(&t)).await;
        }
        clear_token();
        token.set(None);
    });
},
    {icon("logout", 14)}
    "退出登录"
}
```

（main.rs 顶部需 `use crate::api::request;`——已存在。）

- [ ] **Step 6: 构建验证**

Run: `cd crates/server/web && dx build --web --release --debug-symbols false`
Expected: 0 警告，构建成功

- [ ] **Step 7: 全量门禁 + commit**

Run: `cargo fmt --all && cargo clippy --workspace && cargo test --workspace`

```bash
git add crates/web-core/src/dto.rs crates/server/web/src/
git commit -m "feat(web): 登录页双模式（首建引导/登录）+ 配置页账号卡片改密 + 登出服务端注销"
```

---

### Task 6: 文档与 smoke.sh 清理

**Files:**
- Modify: `scripts/smoke.sh`
- Modify: `README.md`
- Modify: `CLAUDE.md`
- Modify: `compose.yaml`

**Interfaces:**
- Consumes: 全部后端/前端改造
- Produces: 端到端验证 `make smoke` 9/9

- [ ] **Step 1: smoke.sh 认证流程改造**

替换「5/9 管理接口」段中从 DB 查 token 的逻辑（保留 401 断言与 config 校验）：

```bash
# ---- 5. 管理接口（login 用同一 Bearer 校验）----
step "5/9 管理接口 /admin/config"
unauth_code="$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$SERVER_PORT/admin/config")"
[[ "$unauth_code" == "401" ]] || fail "无 token 访问 /admin/config 期望 401，实际 $unauth_code"

# 首次运行：引导创建管理员 → 登录拿会话
setup_out="$(curl -sf -X POST "http://127.0.0.1:$SERVER_PORT/admin/setup" \
  -H "Content-Type: application/json" \
  -d '{"username":"smoke","password":"smoke-pass-12345","password_confirm":"smoke-pass-12345"}')"
python3 -c 'import json,sys; d=json.load(sys.stdin); assert d["username"]=="smoke", d; print("setup OK")' <<<"$setup_out"

login_out="$(curl -sf -X POST "http://127.0.0.1:$SERVER_PORT/admin/login" \
  -H "Content-Type: application/json" \
  -d '{"username":"smoke","password":"smoke-pass-12345"}')"
ADMIN_TOKEN="$(python3 -c 'import json,sys; print(json.load(sys.stdin)["token"])' <<<"$login_out")"
[[ -n "$ADMIN_TOKEN" ]] || fail "login 未返回会话 token"

cfg="$(curl -sf "http://127.0.0.1:$SERVER_PORT/admin/config" -H "Authorization: Bearer $ADMIN_TOKEN")"
python3 -c 'import json,sys; d=json.load(sys.stdin); assert d["username"]=="smoke", d; print("config OK")' <<<"$cfg"
printf 'GET /admin/config（Bearer）→ 200 OK, 用户名一致\n'
```

- [ ] **Step 2: README.md 更新**

- 「快速开始」的 token 获取 3 方式 → 替换为：

```markdown
首次访问浏览器打开 `http://<host>:8080`，登录页会引导创建管理员（用户名+密码）。
创建完成后即可登录进入管理界面；之后重启不再出现创建表单。
```

- 环境变量表删除 `SUB_MERGE_ADMIN_TOKEN` 行
- API 表更新：`GET/PUT /admin/config` 描述改为「获取配置（用户名）/ 修改密码」；新增 setup-status/setup/login/logout 行
- 「浏览器打开…输入管理 token，进入管理界面」→「…创建管理员并登录」

- [ ] **Step 3: CLAUDE.md 更新**

- 环境变量表删除 `SUB_MERGE_ADMIN_TOKEN` 行，删除「首次启动日志打印一次随机 token（重启不重复）」说明
- 末段「管理接口一律 `Authorization: Bearer <admin_token>`」→「管理接口一律 `Authorization: Bearer <会话 token>`（登录后获得）」

- [ ] **Step 4: compose.yaml 删环境变量**

```yaml
services:
  sub-merge:
    build: .
    image: sub-merge:latest
    container_name: sub-merge
    ports:
      - "8080:8080"
    volumes:
      - ./submerge-data:/app/data
    restart: unless-stopped
    # environment: SUB_MERGE_ADMIN_TOKEN 整段删除
```

- [ ] **Step 5: 端到端验证**

Run: `make smoke`
Expected: 9/9 全部通过（setup 创建 → login 拿会话 → 订阅输出链路正常）

- [ ] **Step 6: 全量门禁 + commit**

Run: `cargo fmt --all && cargo clippy --workspace && cargo test --workspace`

```bash
git add scripts/smoke.sh README.md CLAUDE.md compose.yaml
git commit -m "chore: 认证改用户名+密码后的文档与冒烟脚本清理"
```

---

## 自审记录

**Spec 覆盖检查：**
- users/sessions 建表、argon2 哈希、会话 sha256 存储 → Task 1
- setup-status / setup（锁定 409 + 校验 400）/ login（统一 401）/ logout（204 幂等）→ Task 2
- require_admin 改查 sessions、AppState 删 token、main.rs 删首启打印、build_router 签名 → Task 3
- ConfigDto username + change_password（旧密码校验、新密码 ≥8、全会话失效）→ Task 4
- 前端双模式、localStorage key 改名、自动登录、账号卡片、登出服务端注销、web-core fixture → Task 5
- SUB_MERGE_ADMIN_TOKEN 删除（compose/README/CLAUDE.md）、smoke.sh → Task 6
- 测试：setup 锁定/校验、login 401、会话访问、logout 失效、改密全失效、UNIQUE 兜底 → Task 1/2/4

**无占位符检查：** 全部步骤含实际代码或明确命令。

**类型一致性：** `build_router(pool, cfg)` 新签名在 Task 3 定义后，Task 4/5 全部使用该签名；db 函数签名在 Task 1 定义后全计划一致（`users_empty`/`create_user`/`verify_user`/`update_password`/`get_username`/`create_session`/`validate_session`/`delete_session`/`delete_all_sessions`）；`extract_bearer` 在 Task 2 定义、Task 3 迁至 auth.rs 并被 logout 引用——Task 3 Step 1 已注明删除 auth.rs 内重复定义。
