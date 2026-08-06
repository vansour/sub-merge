# 源类型与命名组合订阅实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** sources 区分单条节点（`single`，直接解析不拉取）与远程订阅（`remote`，拉取后解析），组合订阅改用命名路径 `GET /subscribe/{name}?format=...` 且无鉴权，彻底移除订阅 token。

**Architecture:** server 的 DB 层加 `kind` 列并迁移旧库；service 层按 kind 分支；路由去 `/api` 前缀（管理端 `/admin/*`、订阅端 `/subscribe/{name}`）；static fallback 守卫扩为 `api|admin|subscribe`；web 前端表单加类型选择、配置页改为组合名 + 无 token 链接。proxy-core 零改动。

**Tech Stack:** axum 0.8 / sqlx 0.9 SQLite / tokio / reqwest / dioxus 0.8.0-alpha.1（web）/ wiremock（测试）

## Global Constraints

- 每次代码修改后必须按顺序全部通过：`cargo upgrade -i`、`cargo fmt --all`、`cargo clippy --workspace`、`cargo test --workspace`（web crate 不在 workspace 内，最后单独 `dx build`）
- Rust edition 2024（根 workspace `edition = "2024"`；web crate 字面 `edition = "2024"`；不得引入旧 edition）
- web crate 独立于 workspace，唯一构建方式：`cd crates/server/web && dx build --web --release`
- dioxus 0.8 alpha 已验证坑：rsx 的 if/else 分支内不能嵌套 `rsx!` 宏（属性表达式无此限制）；`Element` 空渲染用 `VNode::empty()`；`use_effect` 无依赖数组，一次性逻辑用信号守卫；`Signal` 可直接调用取值；svg 属性 snake_case
- 前端 UI 无测试 harness，验证 = `dx build` + `make smoke` + 浏览器人工核对
- 组合订阅名必须匹配 `[A-Za-z0-9-_]`（路径段安全）；`kind` 仅接受 `single|remote`
- 订阅 token 彻底移除：settings 不再写入/读取 `subscribe_token`，环境变量 `SUB_MERGE_SUBSCRIBE_TOKEN` 失效

---

### Task 1: 组合订阅路由 + 去 `/api` 前缀 + static 守卫 + config 接口

路由大改。`ensure_tokens` 签名**本轮不变**（Task 2 改），现有 `let (_, admin)` / `(sub, admin)` 调用点全部照旧。

**Files:**
- Modify: `crates/server/src/routes/mod.rs`
- Modify: `crates/server/src/routes/subscribe.rs`（全文件重写）
- Modify: `crates/server/src/routes/config.rs`
- Modify: `crates/server/src/static.rs:13-16`
- Modify: `crates/server/src/routes/sources.rs:33-41`（仅 router() 路径）
- Modify: `crates/server/src/routes/preview.rs:11-13`（仅 router() 路径）
- Test: `crates/server/tests/api_test.rs`、`crates/server/tests/final_fixes.rs`

**Interfaces:**
- Consumes: 现有 `service::fetch_and_merge(&state) -> (Vec<ProxyNode>, Vec<SourceError>)`、`db::get_setting/set_setting`
- Produces: `GET /subscribe/{name}`（无鉴权）、`GET/PUT /admin/config`（`ConfigDto { admin_token, combined_name, subscribe_url }`）、`ConfigDto.subscribe_url = "/subscribe/{name}"`

- [ ] **Step 1: 重写 subscribe 路由**

`crates/server/src/routes/subscribe.rs` 全文件替换为：

```rust
// crates/server/src/routes/subscribe.rs
use crate::error::ApiError;
use crate::service;
use crate::state::AppState;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::response::Response;
use proxy_core::serializer::{OutputFormat, serialize_nodes};
use std::str::FromStr;

#[derive(serde::Deserialize)]
pub struct SubscribeQuery {
    pub format: Option<String>,
}

pub async fn subscribe_handler(
    State(state): State<AppState>,
    Path(name): Path<String>,
    q: Result<Query<SubscribeQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let Query(q) = q.map_err(ApiError::from)?;
    // 组合订阅名必须匹配 settings 中的 combined_name（缺省 merged）。
    // 不匹配 → 404（区别于 SPA 回退的 HTML 404，走统一 JSON 错误格式）。
    let combined = crate::db::get_setting(&state.pool, "combined_name")
        .await?
        .unwrap_or_else(|| "merged".to_string());
    if name != combined {
        return Err(ApiError::not_found("combined subscription not found"));
    }

    let format = match &q.format {
        Some(f) => {
            OutputFormat::from_str(f).map_err(|_| ApiError::bad_request("unsupported format"))?
        }
        None => OutputFormat::Clash,
    };

    let (nodes, source_errors) = service::fetch_and_merge(&state).await;

    // 若所有源都失败，返回 502 附明细
    if nodes.is_empty() && !source_errors.is_empty() {
        let details = source_errors
            .iter()
            .map(|e| format!("{}: {}", e.source_name, e.reason))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(ApiError::bad_gateway(format!(
            "all sources failed: {details}"
        )));
    }

    let body = serialize_nodes(&nodes, format).map_err(|e| ApiError::internal(e.to_string()))?;

    let content_type = match format {
        OutputFormat::Clash => "application/x-yaml",
        OutputFormat::V2ray => "text/plain; charset=utf-8",
        OutputFormat::Singbox => "application/json",
    };

    let resp = Response::builder()
        .header("content-type", content_type)
        .header("profile-update-interval", "24")
        .body(axum::body::Body::from(body))
        .unwrap();
    Ok(resp)
}
```

- [ ] **Step 2: 改 routes/mod.rs 与其余 router()**

`crates/server/src/routes/mod.rs`（保留子模块声明与 import，替换 build_router 体）：

```rust
pub async fn build_router(
    pool: sqlx::sqlite::SqlitePool,
    cfg: crate::config::AppConfig,
    admin_token: String,
) -> Router {
    let state = AppState::new(pool, cfg, admin_token);
    let api = Router::new()
        .route("/subscribe/{name}", get(subscribe::subscribe_handler))
        .merge(sources::router())
        .merge(preview::router())
        .merge(config::router());

    // 根路径不注册显式路由：由 fallback 返回 SPA index.html（浏览器直接打开 / 即见管理界面）。
    // 健康检查走 /healthz。
    api.route("/healthz", get(|| async { "sub-merge is running" }))
        .fallback(crate::r#static::fallback)
        .with_state(state)
}
```

`crates/server/src/routes/sources.rs` 的 `router()`：`"/api/admin/sources"` → `"/admin/sources"`、`"/api/admin/sources/{id}"` → `"/admin/sources/{id}"`、`"/api/admin/sources/{id}/refresh"` → `"/admin/sources/{id}/refresh"`。

`crates/server/src/routes/preview.rs` 的 `router()`：`"/api/admin/preview"` → `"/admin/preview"`。

- [ ] **Step 3: static.rs 守卫扩展**

`crates/server/src/static.rs:13-16` 处注释与条件替换为：

```rust
    // 未知的 API 命名空间路径（/api/*、/admin/*、/subscribe/*，含 // 双斜杠与编码斜杠形态）
    // 绝不回退到 SPA index.html，返回统一 JSON 404。其余路径（含前端路由）回退 SPA。
    let p = uri.path().trim_start_matches('/');
    if p.starts_with("api") || p.starts_with("admin") || p.starts_with("subscribe") {
        return ApiError::not_found("route not found").into_response();
    }
```

- [ ] **Step 4: config.rs 改造**

`crates/server/src/routes/config.rs` 全文件替换为：

```rust
// crates/server/src/routes/config.rs
use crate::auth::require_admin;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

pub fn router() -> Router<AppState> {
    Router::new().route("/admin/config", get(get_config).put(rotate_config))
}

#[derive(Serialize)]
pub struct ConfigDto {
    pub admin_token: String,
    pub combined_name: String,
    pub subscribe_url: String,
}

#[derive(Deserialize)]
pub struct RotateConfig {
    pub rotate: Option<String>,
    pub combined_name: Option<String>,
}

/// 组合订阅名：路径段安全（无 URL 编码），限定 [A-Za-z0-9-_]
fn valid_combined_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

fn config_dto(state: &AppState) -> Result<ConfigDto, ApiError> {
    let admin = crate::db::get_setting(&state.pool, "admin_token")
        .await?
        .unwrap_or_default();
    let combined_name = crate::db::get_setting(&state.pool, "combined_name")
        .await?
        .unwrap_or_else(|| "merged".to_string());
    Ok(ConfigDto {
        admin_token: admin,
        subscribe_url: format!("/subscribe/{}", combined_name),
        combined_name,
    })
}

async fn get_config(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<ConfigDto>, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    Ok(Json(config_dto(&state)?))
}

async fn rotate_config(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: Result<Json<RotateConfig>, JsonRejection>,
) -> Result<Json<ConfigDto>, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    let Json(body) = body.map_err(ApiError::from)?;
    match body.rotate.as_deref() {
        // 订阅 token 已随订阅 token 移除；rotate 仅接受 admin。
        Some("admin") => {
            let t = crate::db::gen_token();
            crate::db::set_setting(&state.pool, "admin_token", &t).await?;
            state.rotate_admin(t).await;
        }
        Some(_) => {
            return Err(ApiError::bad_request("rotate must be 'admin'"));
        }
        None => {}
    }
    if let Some(n) = &body.combined_name {
        if !valid_combined_name(n) {
            return Err(ApiError::bad_request(
                "combined_name must match [A-Za-z0-9-_]",
            ));
        }
        crate::db::set_setting(&state.pool, "combined_name", n).await?;
    }
    Ok(Json(config_dto(&state)?))
}
```

- [ ] **Step 5: 更新 api_test.rs 的订阅与配置测试**

`crates/server/tests/api_test.rs`：

1. 删除 `subscribe_requires_token` 整个测试（约 121-145 行），替换为：

```rust
#[tokio::test]
async fn subscribe_without_token_succeeds() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-sub-notoken", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let (_, admin) = server::db::ensure_tokens(&pool).await.unwrap();
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
    let (_, admin) = server::db::ensure_tokens(&pool).await.unwrap();
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
    assert!(ct.contains("application/json"), "expected JSON 404, got {ct:?}");
}
```

2. `subscribe_with_valid_token_returns_subscription`（约 147-193 行）：删除 `let (sub, admin)` 中的 `sub` 使用——保持 `let (_, admin)`，URI 改为 `format!("/subscribe/merged?format=clash")`（去掉 `?token={sub}`），其余断言不变。

3. `subscribe_wrong_format_returns_bad_request`（约 195-215 行）：保持 `let (_, admin)`，URI 改为 `"/subscribe/merged?format=bogus"`。

4. `config_get_and_rotate`（约 503-551 行）整体替换为：

```rust
#[tokio::test]
async fn config_get_and_rotate() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-config", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let (_, admin) = server::db::ensure_tokens(&pool).await.unwrap();
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
    assert!(v.get("subscribe_token").is_none(), "subscribe_token must be gone");

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
                .body(Body::from(json!({"combined_name": "bad name!"}).to_string()))
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
    let (_, admin) = server::db::ensure_tokens(&pool).await.unwrap();
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
```

5. `subscribe_skips_unserializable_node_instead_of_500`（约 693-742 行）：保持 `let (sub, admin)` → 改为 `let (_, admin)`，URI 改 `"/subscribe/merged?format=clash"`。

6. `api_path_variants_return_json_404`（约 744-783 行）：路径列表改为：

```rust
    for path in [
        "/api",
        "//api/admin/sources",
        "/api%2Fadmin/preview",
        "/admin",
        "//admin/sources",
        "/admin%2Fsources/preview",
        "/subscribe",
        "//subscribe/whatever",
    ] {
```

（`/subscribe` 无路径段时 axum 不匹配路由、落到 fallback，守卫必须返回 JSON 404。）

- [ ] **Step 6: 更新 final_fixes.rs 路径**

`crates/server/tests/final_fixes.rs`：
- `api_unknown_path_returns_json_404_not_spa`：URI `"/api/does-not-exist"` → `"/admin/does-not-exist"`
- `refresh_source_zero_nodes_reports_ok_false`：URI `"/api/admin/sources/1/refresh"` → `"/admin/sources/1/refresh"`

- [ ] **Step 7: 运行测试验证**

Run: `cargo test --workspace`
Expected: 全绿（subscribe/config/路径相关测试均更新）；若有遗漏的 `/api/admin` 路径断言（编译期不会暴露），按报错逐一改为新路径。

- [ ] **Step 8: fmt + clippy**

Run: `cargo fmt --all && cargo clippy --workspace`
Expected: 无警告。

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "feat(server): named combined subscription /subscribe/{name} without token, drop /api prefix
```
（提交信息结尾加 `Co-Authored-By: Claude <noreply@anthropic.com>`）

---

### Task 2: DB 层——sources.kind 列 + 迁移 + 订阅 token 移除

**Files:**
- Modify: `crates/server/src/db.rs`
- Modify: `crates/server/src/main.rs:19-28`
- Test: `crates/server/tests/api_test.rs`

**Interfaces:**
- Consumes: 无
- Produces: `ensure_tokens(&pool) -> Result<String>`（仅 admin）、`ensure_tokens_with(&pool, initial) -> Result<String>`（initial 收到 `"admin_token"`）、`tokens_initialized(&pool) -> Result<bool>`（仅查 admin_token）、`init_db` 对旧库执行 ALTER 迁移、sources 表含 `kind TEXT NOT NULL DEFAULT 'remote'`

- [ ] **Step 1: db.rs 改造**

`crates/server/src/db.rs`：

1. `init_db` 的 sources 建表 SQL 加 `kind` 列（name 之后）：

```rust
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sources (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            url TEXT NOT NULL,
            name TEXT NOT NULL,
            kind TEXT NOT NULL DEFAULT 'remote',
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL
        )",
    )
    .execute(&pool)
    .await?;
```

2. 建表后追加旧库迁移（列已存在时 ALTER 报错，忽略）：

```rust
    // 旧库迁移：早期版本建表无 kind 列。ALTER 失败（列已存在）忽略。
    let _ = sqlx::query("ALTER TABLE sources ADD COLUMN kind TEXT NOT NULL DEFAULT 'remote'")
        .execute(&pool)
        .await;
```

3. `tokens_initialized` 只查 admin_token：

```rust
/// settings 表是否已初始化 admin token（用于判断是否首次启动、是否需要打印 token）。
pub async fn tokens_initialized(pool: &SqlitePool) -> Result<bool> {
    let row = sqlx::query(
        "SELECT COUNT(*) AS n FROM settings WHERE key = 'admin_token'",
    )
    .fetch_one(pool)
    .await?;
    let n: i64 = row.get(0);
    Ok(n >= 1)
}
```

4. `ensure_tokens` / `ensure_tokens_with` 只返回 admin token（订阅 token 彻底移除）：

```rust
/// 环境变量预设的初始 admin token（仅首次初始化时使用）。
/// 已部署实例的 token 稳定：settings 表已有值时环境变量不生效。
pub async fn ensure_tokens(pool: &SqlitePool) -> Result<String> {
    ensure_tokens_with(pool, |key| {
        std::env::var(format!("SUB_MERGE_{}", key.to_uppercase()))
            .ok()
            .filter(|s| !s.is_empty())
    })
    .await
}

/// 可注入 token 来源的 ensure_tokens（测试用；生产走环境变量预设或随机生成）。
pub async fn ensure_tokens_with(
    pool: &SqlitePool,
    initial: impl Fn(&str) -> Option<String>,
) -> Result<String> {
    let admin = get_setting(pool, "admin_token").await?;
    match admin {
        Some(s) => Ok(s),
        None => {
            let t = initial("admin_token").unwrap_or_else(gen_token);
            set_setting(pool, "admin_token", &t).await?;
            Ok(t)
        }
    }
}
```

- [ ] **Step 2: main.rs 更新**

`crates/server/src/main.rs` 的 token 初始化段（约 18-28 行）替换为：

```rust
    let pool = init_db(&cfg.db_path).await?;
    // 首次初始化时在日志打印 token（warn 级别，仅此一次；之后重启不打印，
    // 避免 token 长期留在公网服务器日志中）
    let first_init = !server::db::tokens_initialized(&pool).await?;
    let admin_token = server::db::ensure_tokens(&pool).await?;
    if first_init {
        tracing::warn!("首次初始化，请妥善保存以下 admin token（仅打印一次）：");
        tracing::warn!("admin_token: {admin_token}");
    }
    // token 是机密，不应在 info 级别输出到日志。仅 debug 级别可见。
    tracing::debug!("admin token: {}", admin_token);
```

- [ ] **Step 3: 重写 api_test.rs 的三个 token 测试**

`crates/server/tests/api_test.rs`：

1. `db_creates_tables_and_tokens`（约 28-43 行）替换为：

```rust
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
    let sub = server::db::get_setting(&pool, "subscribe_token").await.unwrap();
    assert!(sub.is_none(), "subscribe_token must not be initialized");
}
```

2. `env_preset_tokens_used_only_on_first_init`（约 45-80 行）替换为：

```rust
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
    let admin3 = server::db::ensure_tokens_with(&pool2, |_| None).await.unwrap();
    assert_eq!(admin3.len(), 64);
}
```

3. `tokens_initialized_reflects_first_init`（约 82-98 行）：`server::db::ensure_tokens(&pool).await.unwrap();` 两处调用结果不再解构（直接调用即可），其余不变。

- [ ] **Step 4: 更新 api_test.rs 其余 `ensure_tokens` 调用点**

所有 `let (_, admin) = server::db::ensure_tokens(&pool).await.unwrap();` 改为 `let admin = server::db::ensure_tokens(&pool).await.unwrap();`（Task 1 后已无测试使用 `sub`）。涉及测试：`unknown_route_returns_404`、`subscribe_without_token_succeeds`、`subscribe_wrong_combined_name_returns_404`、`subscribe_with_valid_token_returns_subscription`、`subscribe_wrong_format_returns_bad_request`、`fetch_and_merge_respects_concurrency_cap`、`admin_requires_bearer_token`、`admin_crud_sources`、`preview_returns_node_list`、`config_get_and_rotate`、`combined_name_rename_takes_effect`、`admin_token_rotation_takes_effect_live`、`rejection_non_numeric_id_returns_unified_json`、`rejection_malformed_json_returns_unified_json`、`subscribe_skips_unserializable_node_instead_of_500`、`api_path_variants_return_json_404`、`static_index_served_from_dist`。

- [ ] **Step 5: 新增旧库迁移测试**

在 `crates/server/tests/api_test.rs` 末尾追加：

```rust
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
```

- [ ] **Step 6: 运行验证 + 提交**

Run: `cargo test --workspace && cargo fmt --all && cargo clippy --workspace`
Expected: 全绿无警告。

```bash
git add -A
git commit -m "feat(server): sources.kind column with legacy migration, drop subscribe token
```
（提交信息结尾加 `Co-Authored-By: Claude <noreply@anthropic.com>`）

---

### Task 3: service 层按 kind 分支 + sources API kind 字段

**Files:**
- Modify: `crates/server/src/service.rs:19-82`（fetch_and_merge）
- Modify: `crates/server/src/routes/sources.rs`（DTO/创建/更新/刷新）
- Test: `crates/server/tests/api_test.rs`

**Interfaces:**
- Consumes: `sources.kind` 列（Task 2）、`proxy_core::parser::parse_line(&str) -> Result<ProxyNode, ParseError>`、`proxy_core::parser::parse_subscription_text(&str, usize)`
- Produces: `SourceDto.kind: String`；`CreateSource.kind: Option<String>`（缺省 `"remote"`）；`UpdateSource.kind: Option<String>`；`service::fetch_and_merge` 对 `single` 源零网络请求

- [ ] **Step 1: 改 service.rs**

`crates/server/src/service.rs` 的 `fetch_and_merge` 内：SELECT 加 kind、循环内按 kind 分支。具体改动（保持其余结构不变）：

```rust
    let sources: Vec<(i64, String, String, String)> =
        sqlx::query("SELECT id, kind, name, url FROM sources WHERE enabled = 1")
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|r| {
                (
                    r.get::<i64, _>(0),
                    r.get::<String, _>(1),
                    r.get::<String, _>(2),
                    r.get::<String, _>(3),
                )
            })
            .collect();
```

循环体（`for (_, name, url) in sources` 改为 `for (_, kind, name, url) in sources`，spawn 闭包内替换 fetch/parse 逻辑）：

```rust
        let client = client.clone();
        set.spawn(async move {
            let _permit = permit; // 任务期间持有许可
            let result = if kind == "single" {
                // 单条节点：直接解析，不发起网络请求
                proxy_core::parser::parse_line(&url).map(|n| vec![n])
            } else {
                match fetch_source(&client, &url, timeout).await {
                    Ok(text) => {
                        let (nodes, skipped) = parse_subscription_text(&text, max_nodes);
                        if nodes.is_empty() {
                            Err(format!("no nodes parsed ({} line(s) skipped)", skipped))
                        } else {
                            Ok(nodes)
                        }
                    }
                    Err(reason) => Err(reason),
                }
            };
            match result {
                Ok(nodes) => (name, Ok(nodes)),
                Err(reason) => (name, Err(reason)),
            }
        });
```

`parse_line` 的错误为 `ParseError`，`Err(reason)` 处 `reason` 是 `String`——`proxy_core::parser::parse_line(&url).map(...)` 的 Err 是 `ParseError`，与 `Err(reason)` 分支的 String 不匹配，需要 `map_err(|e| format!("parse failed: {e}"))`。即 single 分支为：

```rust
            let result = if kind == "single" {
                // 单条节点：直接解析，不发起网络请求
                proxy_core::parser::parse_line(&url)
                    .map(|n| vec![n])
                    .map_err(|e| format!("parse failed: {e}"))
            } else {
```

- [ ] **Step 2: sources.rs DTO 与 CRUD 加 kind**

`crates/server/src/routes/sources.rs`：

1. `SourceDto` 加字段与 FromRow 查询列：

```rust
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SourceDto {
    pub id: i64,
    pub url: String,
    pub name: String,
    pub kind: String,
    pub enabled: bool,
    pub created_at: String,
}
```

所有 `"SELECT id, url, name, enabled, created_at FROM sources ..."` → `"SELECT id, url, name, kind, enabled, created_at FROM sources ..."`（共 3 处：list、update 的 existing、refresh 的 source）。

2. `CreateSource` 加 `kind: Option<String>`；`create_source` 内：

```rust
    let kind = body.kind.clone().unwrap_or_else(|| "remote".to_string());
    if !matches!(kind.as_str(), "single" | "remote") {
        return Err(ApiError::bad_request("kind must be 'single' or 'remote'"));
    }
    let created_at = chrono::Utc::now().to_rfc3339();
    let res =
        sqlx::query("INSERT INTO sources (url, name, kind, enabled, created_at) VALUES (?, ?, ?, 1, ?)")
            .bind(&body.url)
            .bind(&body.name)
            .bind(&kind)
            .bind(&created_at)
            .execute(&state.pool)
            .await?;
    let id = res.last_insert_rowid();
    let dto = SourceDto {
        id,
        url: body.url,
        name: body.name,
        kind,
        enabled: true,
        created_at,
    };
```

3. `UpdateSource` 加 `kind: Option<String>`；`update_source` 内（existing 之后）：

```rust
    let kind = body.kind.clone().unwrap_or(existing.kind.clone());
    if !matches!(kind.as_str(), "single" | "remote") {
        return Err(ApiError::bad_request("kind must be 'single' or 'remote'"));
    }

    sqlx::query("UPDATE sources SET url = ?, name = ?, kind = ?, enabled = ? WHERE id = ?")
        .bind(&url)
        .bind(&name)
        .bind(&kind)
        .bind(enabled)
        .bind(id)
        .execute(&state.pool)
        .await?;

    let dto = SourceDto {
        id,
        url,
        name,
        kind,
        enabled,
        created_at: existing.created_at,
    };
```

4. `refresh_source`：查询结果带 kind 后，在 `let result = ...` 之前按 kind 短路返回，remote 分支原样保留：

```rust
    // 单条节点：本地重解析，不拉网络
    if source.kind == "single" {
        return match proxy_core::parser::parse_line(&source.url) {
            Ok(_) => Ok(Json(serde_json::json!({
                "source": source.name,
                "ok": true,
                "node_count": 1,
            }))),
            Err(e) => Ok(Json(serde_json::json!({
                "source": source.name,
                "ok": false,
                "reason": format!("parse failed: {e}"),
            }))),
        };
    }
    // 远程订阅：原有 fetch → parse 流程，代码不动
    let result = crate::service::fetch_source(
        &state.http,
        &source.url,
        std::time::Duration::from_secs(state.cfg.timeout_secs),
    )
    .await;
    match result {
        // ... 原 match 分支原样保留 ...
    }
```

注意：`source.kind` 需要 `SourceDto` 从 `fetch_optional` 中读到——`SELECT` 列表加 kind 后 `sqlx::query_as::<_, SourceDto>` 自动填充，无额外改动。

- [ ] **Step 3: 新增测试**

在 `crates/server/tests/api_test.rs` 追加：

```rust
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
    sqlx::query("INSERT INTO sources (url, name, kind, enabled, created_at) VALUES (?, ?, ?, 1, ?)")
        .bind(&url)
        .bind("remote-src")
        .bind("remote")
        .bind("now")
        .execute(&state.pool)
        .await
        .unwrap();
    // single 源：服务器地址 127.0.0.1:1（连接必然失败），若被 fetch 则产生源错误
    sqlx::query("INSERT INTO sources (url, name, kind, enabled, created_at) VALUES (?, ?, ?, 1, ?)")
        .bind("ss://YWVzLTI1Ni1nY206cGFzcw@127.0.0.1:1#SINGLE")
        .bind("single-src")
        .bind("single")
        .bind("now")
        .execute(&state.pool)
        .await
        .unwrap();

    let (nodes, errors) = server::service::fetch_and_merge(&state).await;
    assert!(errors.is_empty(), "expected no source errors, got {errors:?}");
    assert_eq!(nodes.len(), 2);
    assert!(nodes.iter().any(|n| n.name == "SINGLE"), "single node must be parsed");
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

    sqlx::query("INSERT INTO sources (url, name, kind, enabled, created_at) VALUES (?, ?, ?, 1, ?)")
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
    assert!(errors[0].reason.contains("parse failed"), "reason: {}", errors[0].reason);
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

    // 创建 kind=single 源 → 响应与列表都带 kind
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
                .body(Body::from(json!({"url": "https://x/sub", "name": "s2"}).to_string()))
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
```

- [ ] **Step 4: 运行验证 + 提交**

Run: `cargo test --workspace && cargo fmt --all && cargo clippy --workspace`
Expected: 全绿无警告。

```bash
git add -A
git commit -m "feat(server): branch fetch/merge and sources CRUD by source kind
```
（提交信息结尾加 `Co-Authored-By: Claude <noreply@anthropic.com>`）

---

### Task 4: 前端——路径去前缀 + sources 类型选择 + config 组合名与无 token 链接

**Files:**
- Modify: `crates/server/web/src/main.rs:29`（启动校验路径）
- Modify: `crates/server/web/src/components/overview.rs`（fetch 路径两处）
- Modify: `crates/server/web/src/components/preview.rs`（fetch 路径两处）
- Modify: `crates/server/web/src/components/sources.rs`
- Modify: `crates/server/web/src/components/config.rs`
- Modify: `crates/server/web/index.html`（select 样式 + badge.info）

**Interfaces:**
- Consumes: `ConfigDto { admin_token, combined_name, subscribe_url }`（无 subscribe_token）；`SourceDto.kind: String`；`/subscribe/{name}?format=...`
- Produces: sources 表单类型选择；config 页组合名输入与保存；无 token 订阅链接

- [ ] **Step 1: 路径去前缀**

字符串替换（全部前端文件）：
- `"/api/admin/config"` → `"/admin/config"`（main.rs:29、config.rs 两处）
- `"/api/admin/sources"` → `"/admin/sources"`（sources.rs 的 fetch_sources、add、toggle、ask_delete 相关）
- `"/api/admin/sources/{id}"` → `"/admin/sources/{id}"`（format! 拼路径处）
- `"/api/admin/sources/{id}/refresh"` → `"/admin/sources/{id}/refresh"`
- `"/api/admin/preview"` → `"/admin/preview"`（overview.rs、preview.rs 各两处）

- [ ] **Step 2: sources.rs——类型选择与列**

`crates/server/web/src/components/sources.rs`：

1. `SourceDto` 加字段：

```rust
pub struct SourceDto {
    pub id: i64,
    pub url: String,
    pub name: String,
    pub kind: String,
    pub enabled: bool,
    // 后端返回的字段，作为 API 契约保留；UI 表格暂不展示。
    #[allow(dead_code)]
    pub created_at: String,
}
```

2. 状态加 `let mut new_kind = use_signal(|| "remote".to_string());`

3. `add` 闭包 body 改为：

```rust
        let kind = new_kind.read().clone();
        let body = serde_json::json!({ "url": url, "name": name, "kind": kind }).to_string();
```

4. 添加表单（`div { class: "field", label { "订阅 URL" } ... }` 之前插入类型字段）：

```rust
                div { class: "field",
                    label { "类型" }
                    select {
                        value: new_kind,
                        onchange: move |e| new_kind.set(e.value()),
                        option { value: "remote", "远程订阅（订阅链接）" }
                        option { value: "single", "单条节点（URI）" }
                    }
                }
```

URL 输入框 placeholder 改为按类型变化：

```rust
                    input {
                        class: "mono",
                        placeholder: if *new_kind.read() == "single" {
                            "ss://..., vmess://... 单条节点链接"
                        } else {
                            "https://example.com/sub"
                        },
                        value: new_url,
                        oninput: move |e| new_url.set(e.value()),
                    }
```

5. 表格加类型列：thead `tr { th { "名称" } th { "类型" } th { "URL" } th { "状态" } th { "操作" } }`；行内（cell-name 之后）加：

```rust
                    td {
                        span { class: format!("badge {}", if kind == "single" { "info" } else { "off" }),
                            if kind == "single" { "单条" } else { "远程" }
                        }
                    }
```

（行预渲染的 `let url = s.url.clone();` 附近加 `let kind = s.kind.clone();`，`rsx!` 内用 `kind`。）

- [ ] **Step 3: config.rs——组合名 + 无 token 链接 + 去订阅 token**

`crates/server/web/src/components/config.rs`：

1. `ConfigDto` 去掉 `subscribe_token`，加 `combined_name`：

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct ConfigDto {
    pub admin_token: String,
    pub combined_name: String,
    pub subscribe_url: String,
}
```

2. 加状态与加载联动：

```rust
    let mut new_name = use_signal(String::new);
```

`use_future` 的 Ok 分支改为：

```rust
                Ok(body) => match serde_json::from_str::<ConfigDto>(&body) {
                    Ok(c) => {
                        new_name.set(c.combined_name.clone());
                        cfg.set(Some(c));
                    }
                    Err(e) => error.set(format!("解析失败: {}", e)),
                },
```

3. `rotate` 闭包删除 `which` 参数（只轮换 admin）：`rotate` 变为无参，`which == "admin"` 分支恒成立——直接 `write_token(&c.admin_token); token.set(Some(c.admin_token.clone()));`，toast 文案固定 "管理 token 已轮换"。`ask_rotate` 同样去掉参数，只保留 admin 的确认文案（"轮换后旧管理 token 立即失效，当前会话将自动更新为新 token。确定继续？"）。

4. `copy_click` 不变；links 计算改为：

```rust
            [("Clash", "clash"), ("V2Ray", "v2ray"), ("Sing-box", "singbox")]
                .into_iter()
                .map(|(label, fmt)| {
                    let link = format!("{}{}?format={}", base_url, c.subscribe_url, fmt);
                    (label, link)
                })
```

5. 订阅链接卡片下追加组合名卡片（放 `div { class: "card", h2 { "Token" } ... }` 之前）：

```rust
            div { class: "card",
                h2 { class: "card-title", "组合订阅" }
                p { class: "subtle", "组合订阅名决定输出链接路径：/subscribe/{名称}。仅限字母、数字、-、_。" }
                div { class: "form-row",
                    div { class: "field",
                        label { "组合订阅名称" }
                        input {
                            class: "mono",
                            value: new_name,
                            oninput: move |e| new_name.set(e.value()),
                        }
                    }
                    button { class: "btn btn-secondary", onclick: save_name, "保存名称" }
                }
            }
```

`save_name`（`copy_click` 之前定义，`use_callback` 不需要）：

```rust
    let mut save_name = move |_| {
        let name = new_name.read().clone();
        if name.is_empty() {
            error.set("名称不能为空".into());
            return;
        }
        let token = token.read().clone();
        let body = serde_json::json!({ "combined_name": name }).to_string();
        let mut cfg = cfg.clone();
        let mut error = error.clone();
        let mut toasts = toasts.clone();
        spawn(async move {
            match request("PUT", "/admin/config", Some(body), token.as_deref()).await {
                Ok(b) => match serde_json::from_str::<ConfigDto>(&b) {
                    Ok(c) => {
                        cfg.set(Some(c));
                        error.set(String::new());
                        push_toast(toasts, ToastKind::Success, "组合订阅名称已更新");
                    }
                    Err(e) => error.set(format!("解析失败: {}", e)),
                },
                Err(e) => error.set(format!("保存失败: {e}")),
            }
        });
    };
```

6. Token 卡片：删除「订阅 token」token-row 与 subscribe 轮换按钮，只保留管理 token 行（掩码 + 显示/隐藏 + 轮换）。

- [ ] **Step 4: index.html 样式**

`crates/server/web/index.html` 的 `.field input` 规则后追加 select 样式，`.badge.off` 后追加 info：

```css
    .field select {
      width: 100%; background: var(--card); border: 1px solid var(--border); border-radius: 6px;
      padding: 8px 10px; font-size: 14px; color: var(--text); font-family: inherit;
      transition: border-color .15s ease, box-shadow .15s ease;
    }
    .field select:focus {
      outline: none; border-color: var(--accent);
      box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 18%, transparent);
    }
```

```css
    .badge.info { background: var(--accent-soft); color: var(--accent); }
```

- [ ] **Step 5: 构建验证**

Run: `cd crates/server/web && dx build --web --release`
Expected: 构建成功无报错。随后 `make smoke` 通过（Task 5 完成 smoke 更新后最后整体跑）。

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(web): source kind selector, combined subscription name, tokenless links
```
（提交信息结尾加 `Co-Authored-By: Claude <noreply@anthropic.com>`）

---

### Task 5: 文档与冒烟脚本

**Files:**
- Modify: `README.md`
- Modify: `CLAUDE.md`
- Modify: `scripts/smoke.sh`

- [ ] **Step 1: README.md 更新**

1. 功能段：`- 聚合多个订阅源（URL），并发拉取（默认并发 8，单源超时 15s），单源失败自动跳过` 之后追加 `- 两种源类型：单条节点（URI 直接解析，不拉网络）与远程订阅（订阅链接，拉取后解析）；组合订阅使用命名路径输出（无 token 鉴权）`
2. 快速开始段：方式 1/2/3 的 token 描述改为仅管理 token（`SUB_MERGE_ADMIN_TOKEN`）；删除 `SUB_MERGE_SUBSCRIBE_TOKEN` 提及
3. 环境变量表：删除 `SUB_MERGE_SUBSCRIBE_TOKEN` 行
4. API 段：

```markdown
### 订阅接口

```
GET /subscribe/{name}?format=clash|v2ray|singbox
```

`{name}` 为组合订阅名（settings 中 `combined_name`，默认 `merged`，配置页可改）；`format` 缺省为 `clash`。无鉴权。名字不匹配返回 404；全部源失败时返回 502 并附错误明细。

### 管理接口（`Authorization: Bearer <管理token>`）

| 方法 | 路径 | 说明 |
|------|------|------|
| GET/POST | /admin/sources | 列表 / 添加订阅源（`kind`: `single` 单条节点 \| `remote` 远程订阅，缺省 remote） |
| PUT/DELETE | /admin/sources/{id} | 更新（url/name/kind/enabled）/ 删除 |
| POST | /admin/sources/{id}/refresh | 手动刷新单源 |
| GET | /admin/preview | 转换结果预览（节点列表 + 源错误） |
| GET/PUT | /admin/config | 获取配置 / 轮换 admin token、修改组合订阅名 |
```

5. 架构段 `server` 描述中 `双 token 鉴权` → `admin token 鉴权`；`/api/subscribe` → `/subscribe/{name}`
6. "注意：V2Ray 格式的节点覆盖" 段中 `/api/subscribe` 字样随 API 表统一

- [ ] **Step 2: CLAUDE.md 更新**

架构段路由描述改为：

```markdown
- `crates/server`：axum 服务。SQLite（sqlx）持久化 sources/settings。路由：`/subscribe/{name}`（组合订阅输出，无鉴权）、`/admin/*`（源 CRUD/预览/配置，Bearer 鉴权）；`/healthz`；SPA 静态托管 + 前端路由回退。
```

环境变量表中删除 `SUB_MERGE_SUBSCRIBE_TOKEN` 行，`SUB_MERGE_ADMIN_TOKEN` 说明改为 `预设初始 admin token（仅首次初始化时生效，已部署实例不受影响）`。

- [ ] **Step 3: smoke.sh 更新**

`scripts/smoke.sh`：

1. 文件头注释步骤 5-7 描述更新（订阅无 token、/admin 路径）。

2. 步骤 5 替换（原第 5 节整段）：

```bash
# ---- 5. 管理接口（login 用同一 Bearer 校验）----
step "5/9 管理接口 /admin/config"
unauth_code="$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$SERVER_PORT/admin/config")"
[[ "$unauth_code" == "401" ]] || fail "无 token 访问 /admin/config 期望 401，实际 $unauth_code"

# 从日志拿不到 token（debug 级别），直接查 DB 的 settings 表拿 admin token
ADMIN_TOKEN="$(python3 - "$TMP_DIR/submerge-smoke.db" <<'PY'
import sqlite3, sys
db = sqlite3.connect(sys.argv[1])
print(db.execute("SELECT value FROM settings WHERE key='admin_token'").fetchone()[0])
PY
)"
[[ -n "$ADMIN_TOKEN" ]] || fail "DB 中未生成 admin token"
cfg="$(curl -sf "http://127.0.0.1:$SERVER_PORT/admin/config" -H "Authorization: Bearer $ADMIN_TOKEN")"
python3 -c 'import json,sys; d=json.load(sys.stdin); assert d["admin_token"]==sys.argv[1], "admin token 不匹配"; assert d["combined_name"]=="merged", d; assert d["subscribe_url"]=="/subscribe/merged", d; print("config OK")' <<<"$cfg" "$ADMIN_TOKEN"
printf 'GET /admin/config（Bearer）→ 200 OK, token 一致\n'
```

3. 步骤 6 替换（加单条源；订阅无 token）：

```bash
# ---- 6. 加订阅源 → 组合订阅输出 ----
step "6/9 加订阅源 → /subscribe/merged"
created="$(curl -sf -X POST "http://127.0.0.1:$SERVER_PORT/admin/sources" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  -d "{\"url\":\"http://127.0.0.1:$FIXTURE_PORT/sub.txt\",\"name\":\"fixture\"}")"
python3 -c 'import json,sys; d=json.load(sys.stdin); assert d["enabled"]==True; assert d["name"]=="fixture"; assert d["kind"]=="remote", d; print("source id=%d"%d["id"])' <<<"$created"

# 单条节点源（无网络依赖，指向必然失败的地址也不会被拉取）
created_single="$(curl -sf -X POST "http://127.0.0.1:$SERVER_PORT/admin/sources" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"url":"ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#single-node","name":"single","kind":"single"}')"
python3 -c 'import json,sys; d=json.load(sys.stdin); assert d["kind"]=="single", d; print("single source id=%d"%d["id"])' <<<"$created_single"

clash_out="$(curl -sf "http://127.0.0.1:$SERVER_PORT/subscribe/merged?format=clash")"
grep -q "fixture-node" <<<"$clash_out" || fail "/subscribe/merged 未输出 fixture-node"
grep -q "single-node" <<<"$clash_out" || fail "/subscribe/merged 未输出 single-node"
grep -q "proxies:" <<<"$clash_out" || fail "/subscribe/merged 未输出 proxies 段"
printf 'GET /subscribe/merged?format=clash → 200 OK, 含 fixture-node + single-node\n'
```

4. 步骤 7 替换（组合名不匹配 404）：

```bash
# ---- 7. 组合订阅名不匹配 404 ----
step "7/9 错误组合名 404"
wrong_sub="$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$SERVER_PORT/subscribe/not-a-sub?format=clash")"
[[ "$wrong_sub" == "404" ]] || fail "错误组合名期望 404，实际 $wrong_sub"
printf 'GET /subscribe/not-a-sub → 404\n'
```

5. 步骤 8 替换（未知 /admin 路径 JSON 404）：

```bash
# ---- 8. 未知 API 命名空间 404 而非 SPA 回退 ----
step "8/9 未知 /admin/* 返回 JSON 404"
admin404="$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$SERVER_PORT/admin/nope")"
[[ "$admin404" == "404" ]] || fail "未知 /admin/* 期望 404，实际 $admin404"
printf 'GET /admin/nope → 404（不回退 SPA）\n'
```

6. 步骤编号注释同步（原步骤 8 的注释行 "未知 API 404 而非 SPA 回退" 文字调整；总步骤数仍为 9）。

- [ ] **Step 4: 全量验证**

Run: `make smoke`
Expected: 冒烟全部通过（构建前端 + server + 全部 curl 断言）。

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "docs: update README/CLAUDE.md and smoke for named combined subscription
```
（提交信息结尾加 `Co-Authored-By: Claude <noreply@anthropic.com>`）

---

### Task 6: 最终全量验证

**Files:** 无

- [ ] **Step 1: 全量验证命令**

Run（按顺序）:

```bash
cargo upgrade -i
cargo fmt --all
cargo clippy --workspace
cargo test --workspace
cd crates/server/web && dx build --web --release
```

Expected: 全部通过无警告。随后浏览器人工核对（登录 → 订阅源页类型选择 → 配置页组合名改名、链接无 token → 复制链接在客户端/curl 验证 `/subscribe/{新名}?format=clash` 输出）。

- [ ] **Step 2: 浏览器人工核对清单**

1. 登录页正常（admin token）
2. 订阅源页：添加远程订阅（默认）+ 添加单条节点（placeholder 变化）；表格显示类型徽章
3. 配置页：无订阅 token 展示；组合订阅名称输入框保存后链接变化；管理 token 掩码/轮换正常
4. 复制的链接：`{origin}/subscribe/{name}?format=clash`，curl 200 且无 token 参数
