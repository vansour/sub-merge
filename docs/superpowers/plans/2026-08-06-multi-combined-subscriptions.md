# 多组合订阅实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 支持多个组合订阅：每组合从全部源中勾选成员（多对多），`/subscribe/{name}` 按成员输出，组合订阅管理为侧边栏独立页面，预览页可按组合切换。

**Architecture:** 新增 `combined_subs` + `combined_sources` 关联表（外键级联）；`/admin/combineds` CRUD（Bearer）；`service::fetch_and_merge` 增加可选源 id 子集过滤；`/subscribe/{name}` 与 `/admin/preview?combined=` 按成员过滤；前端新增「组合订阅」页面，预览页加组合选择器，配置页移除组合卡片。

**Tech Stack:** axum 0.8 / sqlx 0.9 SQLite / dioxus 0.8.0-alpha.1（web）/ wiremock（测试）

## Global Constraints

- 每次代码修改后必须按顺序全部通过：`cargo upgrade -i`、`cargo fmt --all`、`cargo clippy --workspace`、`cargo test --workspace`（web crate 不在 workspace 内，最后单独 `dx build`）
- Rust edition 2024（根 workspace 与 web crate 均已设置；不得引入旧 edition）
- web crate 独立于 workspace，唯一构建方式：`cd crates/server/web && dx build --web --release`
- dioxus 0.8 alpha 已验证坑：rsx 的 if/else 分支内不能嵌套 `rsx!`（match 分支可编译但属禁区，写元素形式）；`Element` 空渲染用 `VNode::empty()`；`use_effect` 无依赖数组用信号守卫；`Signal` 可直接调用取值；svg 属性 snake_case
- 前端 UI 无测试 harness，验证 = `dx build` + `make smoke` + 浏览器人工核对
- 组合名必须匹配 `[A-Za-z0-9-_]` 且唯一（冲突 400）；改名后旧名 404
- `source_ids` 引用不存在的源：忽略（幂等）
- 成员为空的组合：订阅 200 空输出（不 502）；全部成员源失败：502 附明细
- 旧组合订阅不迁移：settings 的 `combined_name` 残留无害（不再读取）；旧 `/subscribe/merged` 链接 404
- 不要运行 `cargo upgrade -i` 之外的依赖操作；`cargo upgrade -i` 若提议破坏性 major（rand/base64 已是最新兼容，预期无变更），有变更时按上轮处理方式（升级并适配）执行

---

### Task 1: DB 层——组合表 + 外键 pragma

**Files:**
- Modify: `crates/server/src/db.rs`
- Test: `crates/server/tests/api_test.rs`

**Interfaces:**
- Consumes: 无
- Produces: `init_db` 创建 `combined_subs(id, name UNIQUE, created_at)` 与 `combined_sources(combined_id, source_id, PK(combined_id, source_id))`，连接开启 `PRAGMA foreign_keys = ON`（`SqliteConnectOptions::foreign_keys(true)`）；外键 `ON DELETE CASCADE` 双向级联

- [ ] **Step 1: 修改 db.rs**

`crates/server/src/db.rs`：

1. `SqliteConnectOptions` 链式调用加 `.foreign_keys(true)`（`.busy_timeout(...)` 之后）：

```rust
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(std::time::Duration::from_secs(5))
        .foreign_keys(true);
```

2. settings 表建表之后追加两张新表：

```rust
    // 组合订阅：名称唯一；成员为源的子集（多对多）
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS combined_subs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            created_at TEXT NOT NULL
        )",
    )
    .execute(&pool)
    .await?;

    // 多对多关联：删组合/删源均级联清理（依赖 foreign_keys = ON）
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS combined_sources (
            combined_id INTEGER NOT NULL REFERENCES combined_subs(id) ON DELETE CASCADE,
            source_id INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
            PRIMARY KEY (combined_id, source_id)
        )",
    )
    .execute(&pool)
    .await?;
```

- [ ] **Step 2: 新增级联测试**

`crates/server/tests/api_test.rs` 末尾追加：

```rust
#[tokio::test]
async fn combined_tables_and_cascade() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-combined-tbl", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;

    // 建一个源 + 两个组合，源被两个组合共享
    let res = sqlx::query("INSERT INTO sources (url, name, enabled, created_at) VALUES ('ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#S', 's', 1, 'now')")
        .execute(&pool).await.unwrap();
    let src_id = res.last_insert_rowid();
    let res = sqlx::query("INSERT INTO combined_subs (name, created_at) VALUES ('a', 'now')")
        .execute(&pool).await.unwrap();
    let ca = res.last_insert_rowid();
    let res = sqlx::query("INSERT INTO combined_subs (name, created_at) VALUES ('b', 'now')")
        .execute(&pool).await.unwrap();
    let cb = res.last_insert_rowid();
    for cid in [ca, cb] {
        sqlx::query("INSERT INTO combined_sources (combined_id, source_id) VALUES (?, ?)")
            .bind(cid).bind(src_id).execute(&pool).await.unwrap();
    }

    // 删除源 → 两个组合的成员关系都被级联清理
    sqlx::query("DELETE FROM sources WHERE id = ?").bind(src_id).execute(&pool).await.unwrap();
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM combined_sources WHERE source_id = ?")
        .bind(src_id).fetch_one(&pool).await.unwrap();
    assert_eq!(n, 0, "source deletion must cascade to combined_sources");

    // 删组合 a → 其成员关系清理（b 不受影响——重新插回源再验证）
    let res = sqlx::query("INSERT INTO sources (url, name, enabled, created_at) VALUES ('ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#S2', 's2', 1, 'now')")
        .execute(&pool).await.unwrap();
    let src2 = res.last_insert_rowid();
    for cid in [ca, cb] {
        sqlx::query("INSERT INTO combined_sources (combined_id, source_id) VALUES (?, ?)")
            .bind(cid).bind(src2).execute(&pool).await.unwrap();
    }
    sqlx::query("DELETE FROM combined_subs WHERE id = ?").bind(ca).execute(&pool).await.unwrap();
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM combined_sources WHERE combined_id = ?")
        .bind(ca).fetch_one(&pool).await.unwrap();
    assert_eq!(n, 0, "combined deletion must cascade members");
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM combined_sources WHERE combined_id = ?")
        .bind(cb).fetch_one(&pool).await.unwrap();
    assert_eq!(n, 1, "other combined must keep its member");
}
```

- [ ] **Step 3: 运行验证 + 提交**

Run: `cargo test -p server --test api_test combined_tables_and_cascade` → 通过；随后 `cargo fmt --all && cargo clippy --workspace && cargo test --workspace` 全绿。

```bash
git add -A
git commit -m "feat(server): combined_subs/combined_sources tables with FK cascade
```
（提交信息结尾加 `Co-Authored-By: Claude <noreply@anthropic.com>`）

---

### Task 2: 组合 CRUD API + 共享名字校验 + 配置接口精简（后端）

**Files:**
- Create: `crates/server/src/routes/combineds.rs`
- Modify: `crates/server/src/routes/mod.rs`（注册 combineds::router、`pub(crate) fn valid_combined_name`）
- Modify: `crates/server/src/routes/config.rs`（移除 combined_name/subscribe_url、valid_combined_name 改引用）
- Test: `crates/server/tests/api_test.rs`

**Interfaces:**
- Consumes: `db::gen_token`（config.rs 轮换）、`require_admin`（auth.rs）、Task 1 的两张表
- Produces: `GET/POST /admin/combineds`、`PUT/DELETE /admin/combineds/{id}`；`CombinedDto { id, name, created_at, source_ids: Vec<i64> }`；`pub(crate) fn valid_combined_name(&str) -> bool`（routes/mod.rs）；`ConfigDto { admin_token }`（无 combined_name/subscribe_url）

- [ ] **Step 1: routes/mod.rs 加共享校验函数**

`crates/server/src/routes/mod.rs` 顶部（`pub mod` 声明之后）加：

```rust
/// 组合订阅名：路径段安全（无 URL 编码），限定 [A-Za-z0-9-_]
pub(crate) fn valid_combined_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}
```

并加 `pub mod combineds;` 声明；`build_router` 中 `.merge(sources::router())` 之前加 `.merge(combineds::router())`。

- [ ] **Step 2: 新建 combineds.rs**

`crates/server/src/routes/combineds.rs` 完整内容：

```rust
// crates/server/src/routes/combineds.rs
use crate::auth::require_admin;
use crate::error::ApiError;
use crate::routes::valid_combined_name;
use crate::state::AppState;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct CombinedDto {
    pub id: i64,
    pub name: String,
    pub created_at: String,
    // 成员 source_id 列表（服务端查询填充）
    pub source_ids: Vec<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CreateCombined {
    pub name: String,
    pub source_ids: Option<Vec<i64>>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateCombined {
    pub name: Option<String>,
    pub source_ids: Option<Vec<i64>>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/combineds", get(list_combineds).post(create_combined))
        .route(
            "/admin/combineds/{id}",
            put(update_combined).delete(delete_combined),
        )
}

/// SQLite UNIQUE 约束冲突（错误码 2067 / 消息含 UNIQUE constraint failed）
fn is_unique_violation(e: &sqlx::Error) -> bool {
    e.as_database_error()
        .map(|d| d.message().contains("UNIQUE"))
        .unwrap_or(false)
}

async fn member_ids(state: &AppState, combined_id: i64) -> Result<Vec<i64>, ApiError> {
    Ok(sqlx::query_scalar(
        "SELECT source_id FROM combined_sources WHERE combined_id = ? ORDER BY source_id",
    )
    .bind(combined_id)
    .fetch_all(&state.pool)
    .await?)
}

/// 插入成员：跳过不存在的源 id（幂等）；PK 冲突用 INSERT OR IGNORE。
async fn insert_members(
    state: &AppState,
    combined_id: i64,
    source_ids: &[i64],
) -> Result<(), ApiError> {
    for sid in source_ids {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sources WHERE id = ?)")
            .bind(sid)
            .fetch_one(&state.pool)
            .await?;
        if exists {
            sqlx::query(
                "INSERT OR IGNORE INTO combined_sources (combined_id, source_id) VALUES (?, ?)",
            )
            .bind(combined_id)
            .bind(sid)
            .execute(&state.pool)
            .await?;
        }
    }
    Ok(())
}

fn dto(id: i64, name: String, created_at: String, source_ids: Vec<i64>) -> CombinedDto {
    CombinedDto {
        id,
        name,
        created_at,
        source_ids,
    }
}

async fn list_combineds(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Vec<CombinedDto>>, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    let rows: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT id, name, created_at FROM combined_subs ORDER BY id",
    )
    .fetch_all(&state.pool)
    .await?;
    let mut out = Vec::new();
    for (id, name, created_at) in rows {
        out.push(dto(id, name, created_at, member_ids(&state, id).await?));
    }
    Ok(Json(out))
}

async fn create_combined(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: Result<Json<CreateCombined>, JsonRejection>,
) -> Result<(axum::http::StatusCode, Json<CombinedDto>), ApiError> {
    require_admin(State(state.clone()), headers).await?;
    let Json(body) = body.map_err(ApiError::from)?;
    if !valid_combined_name(&body.name) {
        return Err(ApiError::bad_request(
            "combined name must match [A-Za-z0-9-_]",
        ));
    }
    let created_at = chrono::Utc::now().to_rfc3339();
    let res = sqlx::query("INSERT INTO combined_subs (name, created_at) VALUES (?, ?)")
        .bind(&body.name)
        .bind(&created_at)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            if is_unique_violation(&e) {
                ApiError::bad_request("combined name already exists")
            } else {
                e.into()
            }
        })?;
    let id = res.last_insert_rowid();
    let source_ids = body.source_ids.unwrap_or_default();
    insert_members(&state, id, &source_ids).await?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(dto(id, body.name, created_at, source_ids)),
    ))
}

async fn update_combined(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    id: Result<Path<i64>, PathRejection>,
    body: Result<Json<UpdateCombined>, JsonRejection>,
) -> Result<Json<CombinedDto>, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    let Path(id) = id.map_err(ApiError::from)?;
    let Json(body) = body.map_err(ApiError::from)?;

    // 组合必须存在（先查，区分 404 与校验 400）
    let existing: Option<(String, String)> =
        sqlx::query_as("SELECT name, created_at FROM combined_subs WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| ApiError::not_found("combined subscription not found"))?;
    let (old_name, created_at) = existing;

    // 名字更新：校验 + 唯一性
    let name = match &body.name {
        Some(n) => {
            if !valid_combined_name(n) {
                return Err(ApiError::bad_request(
                    "combined name must match [A-Za-z0-9-_]",
                ));
            }
            if n != &old_name {
                sqlx::query("UPDATE combined_subs SET name = ? WHERE id = ?")
                    .bind(n)
                    .bind(id)
                    .execute(&state.pool)
                    .await
                    .map_err(|e| {
                        if is_unique_violation(&e) {
                            ApiError::bad_request("combined name already exists")
                        } else {
                            e.into()
                        }
                    })?;
            }
            n.clone()
        }
        None => old_name,
    };

    // 成员全量替换（事务：删除 + 插入，避免中间态）
    if let Some(ids) = &body.source_ids {
        let mut tx = state.pool.begin().await?;
        sqlx::query("DELETE FROM combined_sources WHERE combined_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        for sid in ids {
            let exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sources WHERE id = ?)")
                    .bind(sid)
                    .fetch_one(&mut *tx)
                    .await?;
            if exists {
                sqlx::query(
                    "INSERT OR IGNORE INTO combined_sources (combined_id, source_id) VALUES (?, ?)",
                )
                .bind(id)
                .bind(sid)
                .execute(&mut *tx)
                .await?;
            }
        }
        tx.commit().await?;
    }

    Ok(Json(dto(id, name, created_at, member_ids(&state, id).await?)))
}

async fn delete_combined(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    id: Result<Path<i64>, PathRejection>,
) -> Result<axum::http::StatusCode, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    let Path(id) = id.map_err(ApiError::from)?;
    let res = sqlx::query("DELETE FROM combined_subs WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(ApiError::not_found("combined subscription not found"));
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}
```

注意：`sqlx::query_scalar(...).fetch_one(&mut *tx)` 对事务的调用形态（`&mut *tx`），与现有代码模式一致。

- [ ] **Step 3: config.rs 精简**

`crates/server/src/routes/config.rs`：

1. 删除 `ConfigDto` 的 `combined_name` 与 `subscribe_url` 字段（只留 `admin_token`）；`RotateConfig` 删除 `combined_name` 字段
2. 删除 `valid_combined_name` 函数（改用 `crate::routes::valid_combined_name`——不再被引用则整体删除）
3. `config_dto` 只查 admin_token；`rotate_config` 删除 combined_name 校验与写入分支；`subscribe_url` 相关全部移除
4. 确认 `config_dto` 仍为 async fn（保留 `.await?` 形态）

- [ ] **Step 4: 更新 api_test.rs 的组合/配置测试**

`crates/server/tests/api_test.rs`：

1. `config_get_and_rotate`：删除 `combined_name == "merged"`、`subscribe_url` 断言与 `{"combined_name": "my-sub"}` 保存/校验分支（400 断言）；保留 `subscribe_token` 不存在断言与 `rotate: "subscribe"` → 400、`rotate: "admin"` 轮换断言（若存在）。
2. `combined_name_rename_takes_effect` 整个删除（改名语义由组合 CRUD 测试覆盖，见 Step 5）。
3. 新增测试：

```rust
#[tokio::test]
async fn combined_crud_and_members() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-combined-crud", std::process::id()));
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

    // 两个源
    let srcs: Vec<i64> = {
        let mut v = Vec::new();
        for (name, url) in [("s1", "ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#S1"), ("s2", "ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#S2")] {
            let resp = app.clone().oneshot(auth(
                Request::builder()
                    .method("POST")
                    .uri("/admin/sources")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"url": url, "name": name, "kind": "single"}).to_string()))
                    .unwrap(),
            )).await.unwrap();
            let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
            let v0: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            v.push(v0["id"].as_i64().unwrap());
        }
        v
    };

    // 创建组合（勾选 s1；s2 不选；另给一个不存在的 id 999 → 忽略）
    let resp = app.clone().oneshot(auth(
        Request::builder()
            .method("POST")
            .uri("/admin/combineds")
            .header("content-type", "application/json")
            .body(Body::from(json!({"name": "my-sub", "source_ids": [srcs[0], 999]}).to_string()))
            .unwrap(),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["name"], "my-sub");
    assert_eq!(v["source_ids"], json!([srcs[0]]), "nonexistent source id must be ignored");
    let cid = v["id"].as_i64().unwrap();

    // 列表
    let resp = app.clone().oneshot(auth(
        Request::builder().uri("/admin/combineds").body(Body::empty()).unwrap(),
    )).await.unwrap();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let list: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["source_ids"], json!([srcs[0]]));

    // 成员全量替换为 [s2]
    let resp = app.clone().oneshot(auth(
        Request::builder()
            .method("PUT")
            .uri(format!("/admin/combineds/{}", cid))
            .header("content-type", "application/json")
            .body(Body::from(json!({"source_ids": [srcs[1]]}).to_string()))
            .unwrap(),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["source_ids"], json!([srcs[1]]), "members must be fully replaced");

    // 改名
    let resp = app.clone().oneshot(auth(
        Request::builder()
            .method("PUT")
            .uri(format!("/admin/combineds/{}", cid))
            .header("content-type", "application/json")
            .body(Body::from(json!({"name": "renamed-sub"}).to_string()))
            .unwrap(),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["name"], "renamed-sub");

    // 名字冲突 → 400；非法名字 → 400
    let resp = app.clone().oneshot(auth(
        Request::builder()
            .method("POST")
            .uri("/admin/combineds")
            .header("content-type", "application/json")
            .body(Body::from(json!({"name": "renamed-sub"}).to_string()))
            .unwrap(),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let resp = app.clone().oneshot(auth(
        Request::builder()
            .method("POST")
            .uri("/admin/combineds")
            .header("content-type", "application/json")
            .body(Body::from(json!({"name": "bad name!"}).to_string()))
            .unwrap(),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 删除 → 404（不存在）
    let resp = app.clone().oneshot(auth(
        Request::builder()
            .method("DELETE")
            .uri(format!("/admin/combineds/{}", cid))
            .body(Body::empty())
            .unwrap(),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let resp = app.clone().oneshot(auth(
        Request::builder()
            .method("DELETE")
            .uri(format!("/admin/combineds/{}", cid))
            .body(Body::empty())
            .unwrap(),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
```

- [ ] **Step 5: 运行验证 + 提交**

Run: `cargo fmt --all && cargo clippy --workspace && cargo test --workspace` 全绿。

```bash
git add -A
git commit -m "feat(server): combined subscription CRUD at /admin/combineds, slim config DTO
```
（提交信息结尾加 `Co-Authored-By: Claude <noreply@anthropic.com>`）

---

### Task 3: 订阅/预览按组合过滤 + service 子集

**Files:**
- Modify: `crates/server/src/service.rs`（fetch_and_merge 加 `Option<&[i64]>`）
- Modify: `crates/server/src/routes/subscribe.rs`
- Modify: `crates/server/src/routes/preview.rs`
- Test: `crates/server/tests/api_test.rs`

**Interfaces:**
- Consumes: Task 1 的表、Task 2 的 `CombinedDto` 语义
- Produces: `service::fetch_and_merge(&state, Option<&[i64]>) -> (Vec<ProxyNode>, Vec<SourceError>)`；`GET /subscribe/{name}` 按成员输出；`GET /admin/preview?combined=<name>`

- [ ] **Step 1: service.rs 加子集过滤**

`fetch_and_merge` 签名与查询改造：

```rust
/// 并发拉取 enabled 源（受 cfg.concurrency 上限约束），解析合并。
/// source_ids = Some(ids) 时仅拉取指定源的子集（组合订阅）；None 拉取全部。
/// 返回 (节点, 错误源列表)。
pub async fn fetch_and_merge(
    state: &AppState,
    source_ids: Option<&[i64]>,
) -> (Vec<ProxyNode>, Vec<SourceError>) {
    let sources: Vec<(i64, String, String, String)> = match source_ids {
        Some(ids) if !ids.is_empty() => {
            // 组合成员子集：动态 IN 子句
            let placeholders = std::iter::repeat("?")
                .take(ids.len())
                .collect::<Vec<_>>()
                .join(",");
            let mut q = sqlx::query(&format!(
                "SELECT id, kind, name, url FROM sources WHERE enabled = 1 AND id IN ({placeholders})"
            ));
            for id in ids {
                q = q.bind(id);
            }
            q.fetch_all(&state.pool).await.unwrap_or_default()
        }
        // 空成员组合：无源可拉
        Some(_) => Vec::new(),
        None => {
            sqlx::query("SELECT id, kind, name, url FROM sources WHERE enabled = 1")
                .fetch_all(&state.pool)
                .await
                .unwrap_or_default()
        }
    }
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
    // 其余逻辑不变……
```

（`Some(&[])` 返回空节点 → 订阅 200 空输出。）

现有直接调用 `fetch_and_merge(&state)` 的测试（`fetch_and_merge_respects_concurrency_cap`、`single_source_parses_without_network`、`invalid_single_source_reports_source_error`）改为 `fetch_and_merge(&state, None)`。

- [ ] **Step 2: subscribe.rs 按组合查成员**

`crates/server/src/routes/subscribe.rs` 中 combined_name 校验段替换为：

```rust
    // 组合订阅名必须匹配 combined_subs 表；不匹配 → 404
    let combined: Option<(i64, String)> =
        sqlx::query_as("SELECT id, name FROM combined_subs WHERE name = ?")
            .bind(&name)
            .fetch_optional(&state.pool)
            .await
            .map_err(ApiError::from)?;
    let Some((combined_id, _)) = combined else {
        return Err(ApiError::not_found("combined subscription not found"));
    };
    // 成员源 id 列表（空成员 → 空输出，200）
    let member_ids: Vec<i64> =
        sqlx::query_scalar("SELECT source_id FROM combined_sources WHERE combined_id = ? ORDER BY source_id")
            .bind(combined_id)
            .fetch_all(&state.pool)
            .await
            .map_err(ApiError::from)?;

    let format = match &q.format { /* 不变 */ };

    let (nodes, source_errors) = service::fetch_and_merge(&state, Some(&member_ids)).await;
```

删除 `crate::db::get_setting` 的 combined_name 读取；import 若有 unused 一并清理。

- [ ] **Step 3: preview.rs 加 combined 参数**

`crates/server/src/routes/preview.rs`：

```rust
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};

#[derive(serde::Deserialize)]
pub struct PreviewQuery {
    pub combined: Option<String>,
}

pub async fn preview_handler(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    q: Result<Query<PreviewQuery>, QueryRejection>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    let Query(q) = q.map_err(ApiError::from)?;
    // combined 参数：按组合成员过滤；省略 → 全部 enabled 源
    let member_ids: Option<Vec<i64>> = match &q.combined {
        Some(name) => {
            let cid: Option<i64> =
                sqlx::query_scalar("SELECT id FROM combined_subs WHERE name = ?")
                    .bind(name)
                    .fetch_optional(&state.pool)
                    .await?;
            let Some(cid) = cid else {
                return Err(ApiError::not_found("combined subscription not found"));
            };
            Some(
                sqlx::query_scalar(
                    "SELECT source_id FROM combined_sources WHERE combined_id = ? ORDER BY source_id",
                )
                .bind(cid)
                .fetch_all(&state.pool)
                .await?,
            )
        }
        None => None,
    };
    let (nodes, errors) = service::fetch_and_merge(&state, member_ids.as_deref()).await;
    // 以下 node_list/error_list 构造与 json! 响应体保持现有实现不变
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
    let error_list: Vec<String> = errors
        .iter()
        .map(|e| format!("{}: {}", e.source_name, e.reason))
        .collect();
    Ok(Json(json!({
        "nodes": node_list,
        "errors": error_list,
        "total": nodes.len(),
    })))
}
```

- [ ] **Step 4: 更新订阅相关测试**

`crates/server/tests/api_test.rs`：

1. `subscribe_returns_subscription`：INSERT 改为保留 `last_insert_rowid()`，建组合后订阅。改动：

```rust
    // 插入一个指向 mock server 的源
    let url = format!("{}/sub", mock.uri());
    let res = sqlx::query("INSERT INTO sources (url, name, enabled, created_at) VALUES (?, ?, 1, ?)")
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
```

URI 改为 `"/subscribe/merged?format=clash"`（已无 token），其余断言不变。

2. `subscribe_skips_unserializable_node_instead_of_500`：同样的 INSERT 改造（保留 `last_insert_rowid()` → 建 `merged` 组合 → 勾选），URI 改为 `"/subscribe/merged?format=clash"`，其余断言不变。
3. 新增测试：

```rust
#[tokio::test]
async fn combined_subscription_serves_only_members() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/sub"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#IN\n"))
        .mount(&mock)
        .await;

    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-combined-sub", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let admin = server::db::ensure_tokens(&pool).await.unwrap();
    let url = format!("{}/sub", mock.uri());
    // remote 源（mock，节点 IN）与 single 源（节点 OUT，指向不可达地址）
    let res = sqlx::query("INSERT INTO sources (url, name, kind, enabled, created_at) VALUES (?, ?, ?, 1, ?)")
        .bind(&url).bind("in-src").bind("remote").bind("now")
        .execute(&pool).await.unwrap();
    let in_id = res.last_insert_rowid();
    sqlx::query("INSERT INTO sources (url, name, kind, enabled, created_at) VALUES (?, ?, ?, 1, ?)")
        .bind("ss://YWVzLTI1Ni1nY206cGFzcw@127.0.0.1:1#OUT").bind("out-src").bind("single").bind("now")
        .execute(&pool).await.unwrap();

    // 组合只勾选 in-src
    sqlx::query("INSERT INTO combined_subs (name, created_at) VALUES ('grp', 'now')")
        .execute(&pool).await.unwrap();
    let cid: i64 = sqlx::query_scalar("SELECT id FROM combined_subs WHERE name = 'grp'")
        .fetch_one(&pool).await.unwrap();
    sqlx::query("INSERT INTO combined_sources (combined_id, source_id) VALUES (?, ?)")
        .bind(cid).bind(in_id).execute(&pool).await.unwrap();

    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg, admin).await;

    let resp = app.clone().oneshot(
        Request::builder().uri("/subscribe/grp?format=clash").body(Body::empty()).unwrap(),
    ).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(body.contains("name: IN"), "member node must be present");
    assert!(!body.contains("OUT"), "non-member must be excluded");
}

#[tokio::test]
async fn combined_subscription_empty_members_returns_200() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-combined-empty", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let admin = server::db::ensure_tokens(&pool).await.unwrap();
    sqlx::query("INSERT INTO combined_subs (name, created_at) VALUES ('empty-grp', 'now')")
        .execute(&pool).await.unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg, admin).await;

    let resp = app.oneshot(
        Request::builder().uri("/subscribe/empty-grp?format=clash").body(Body::empty()).unwrap(),
    ).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "empty members must be 200, not 502");
}

#[tokio::test]
async fn preview_combined_filter() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-preview-cmb", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let admin = server::db::ensure_tokens(&pool).await.unwrap();
    let res = sqlx::query("INSERT INTO sources (url, name, kind, enabled, created_at) VALUES ('ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#S1', 's1', 'single', 1, 'now')")
        .execute(&pool).await.unwrap();
    let src = res.last_insert_rowid();
    sqlx::query("INSERT INTO combined_subs (name, created_at) VALUES ('grp', 'now')")
        .execute(&pool).await.unwrap();
    let cid: i64 = sqlx::query_scalar("SELECT id FROM combined_subs WHERE name = 'grp'")
        .fetch_one(&pool).await.unwrap();
    sqlx::query("INSERT INTO combined_sources (combined_id, source_id) VALUES (?, ?)")
        .bind(cid).bind(src).execute(&pool).await.unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg, admin.clone()).await;

    let auth = |mut req: Request<Body>| {
        req.headers_mut().insert("authorization", format!("Bearer {}", admin).parse().unwrap());
        req
    };

    // 按组合过滤
    let resp = app.clone().oneshot(auth(
        Request::builder().uri("/admin/preview?combined=grp").body(Body::empty()).unwrap(),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["total"], 1);
    assert_eq!(v["nodes"][0]["name"], "S1");

    // 不存在的组合 → 404
    let resp = app.clone().oneshot(auth(
        Request::builder().uri("/admin/preview?combined=nope").body(Body::empty()).unwrap(),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // 省略参数 → 全部源
    let resp = app.oneshot(auth(
        Request::builder().uri("/admin/preview").body(Body::empty()).unwrap(),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["total"], 1);
}
```

- [ ] **Step 5: 运行验证 + 提交**

Run: `cargo fmt --all && cargo clippy --workspace && cargo test --workspace` 全绿。

```bash
git add -A
git commit -m "feat(server): subscribe and preview scoped to combined subscription members
```
（提交信息结尾加 `Co-Authored-By: Claude <noreply@anthropic.com>`）

---

### Task 4: 前端——组合订阅页（导航 + 组件 + 图标）

**Files:**
- Modify: `crates/server/web/src/main.rs`（导航项 + tab 索引）
- Create: `crates/server/web/src/components/combineds.rs`
- Modify: `crates/server/web/src/components/mod.rs`（声明 + `pub async fn copy_text` 共享）
- Modify: `crates/server/web/src/components/icon.rs`（新增 "combineds" 图标）
- Modify: `crates/server/web/src/components/config.rs`（copy_text 改用共享函数）

**Interfaces:**
- Consumes: `request`（api.rs）、`fetch_sources`/`SourceDto`（sources.rs）、`ConfirmDialog`/`ConfirmState`（confirm.rs）、`push_toast`/`use_toast`/`ToastKind`（toast.rs）、`copy_text`（components/mod.rs，本任务创建）
- Produces: `GET /admin/combineds` 前端 `fetch_combineds() -> Vec<CombinedDto>`；组合页 UI；侧边栏「组合订阅」导航项

- [ ] **Step 1: components/mod.rs 共享 copy_text**

`crates/server/web/src/components/mod.rs` 追加：

```rust
// 剪贴板写入（config 页与组合订阅页共用）。web-sys 0.3.103 实测签名：
//   Window::navigator() -> Navigator（直接返回）；Navigator::clipboard() -> Clipboard
//   Clipboard::write_text(&str) -> js_sys::Promise（用 JsFuture await）
pub async fn copy_text(text: String) -> Result<(), String> {
    let nav = web_sys::window().map(|w| w.navigator()).ok_or("无窗口")?;
    let clip = nav.clipboard();
    wasm_bindgen_futures::JsFuture::from(clip.write_text(&text))
        .await
        .map_err(|e| format!("{:?}", e))?;
    Ok(())
}
```

`config.rs`：删除本地 `copy_text` 定义，`use crate::components::copy_text;`（同时清理 `use wasm_bindgen_futures::JsFuture;` 若不再使用）。

- [ ] **Step 2: icon.rs 加图标**

`crates/server/web/src/components/icon.rs` 的 match 中（"sources" 分支后）加：

```rust
        // combineds：层叠（多个组合）
        "combineds" => rsx! {
            svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
                path { d: "M12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.9a1 1 0 0 0 0-1.83Z" }
                path { d: "m22 17.65-9.17 4.16a2 2 0 0 1-1.66 0L2 17.65" }
                path { d: "m22 12.65-9.17 4.16a2 2 0 0 1-1.66 0L2 12.65" }
            }
        },
```

- [ ] **Step 3: main.rs 导航**

`crates/server/web/src/main.rs` 的 `MainShell`：

1. nav 中 `NavItem { name: "sources", ... }` 之后插入：

```rust
                    NavItem { name: "combineds", label: "组合订阅", active: *tab.read() == 2, onnav: move |_| tab.set(2) }
```

2. 后续三个 NavItem 的索引与 `match *tab.read()` 分支同步 +1：

```rust
                    NavItem { name: "preview", label: "预览", active: *tab.read() == 3, onnav: move |_| tab.set(3) }
                    NavItem { name: "config", label: "配置", active: *tab.read() == 4, onnav: move |_| tab.set(4) }
```

3. 页面 match：

```rust
                    match *tab.read() {
                        0 => rsx! { Overview { token, on_goto } },
                        1 => rsx! { Sources { token } },
                        2 => rsx! { Combineds { token } },
                        3 => rsx! { Preview { token } },
                        _ => rsx! { Config { token } },
                    }
```

4. overview.rs 中"管理订阅源"按钮 `on_goto.call(1)` 不变（仍是 sources tab）。

- [ ] **Step 4: combineds.rs 组件**

`crates/server/web/src/components/combineds.rs` 完整内容：

```rust
// 组合订阅页：组合列表（成员数 + 三种格式链接复制）+ 新建/编辑弹窗（名字 + 成员勾选）。
use crate::api::request;
use crate::components::confirm::{ConfirmDialog, ConfirmState};
use crate::components::copy_text;
use crate::components::icon::{icon, Spinner};
use crate::components::sources::{fetch_sources, SourceDto};
use crate::components::toast::{push_toast, schedule_timeout, use_toast, ToastKind};
use dioxus::prelude::*;
use serde::Deserialize;
use std::rc::Rc;

#[derive(Debug, Clone, Deserialize)]
pub struct CombinedDto {
    pub id: i64,
    pub name: String,
    pub created_at: String,
    pub source_ids: Vec<i64>,
}

pub async fn fetch_combineds(token: Option<&str>) -> Result<Vec<CombinedDto>, String> {
    let body = request("GET", "/admin/combineds", None, token).await?;
    serde_json::from_str(&body).map_err(|e| format!("解析失败: {}", e))
}

// 弹窗表单状态：None = 关闭；Some(edit_id) = 编辑既有组合（name 预填）；新建时 Some(-1)
#[derive(Debug, Clone, Default)]
struct FormState {
    open: bool,
    edit_id: Option<i64>,
    name: String,
    checked: Vec<i64>,
}

#[component]
pub fn Combineds(token: Signal<Option<String>>) -> Element {
    let combineds = use_signal(Vec::<CombinedDto>::new);
    let sources = use_signal(Vec::<SourceDto>::new);
    let mut error = use_signal(String::new);
    let mut form = use_signal(FormState::default);
    let mut saving = use_signal(|| false);
    let mut confirm = use_signal(ConfirmState::default);
    let mut pending_id = use_signal(|| None::<i64>);
    let mut copied = use_signal(|| None::<String>);
    let toasts = use_toast();

    // 初次挂载加载组合与源列表
    use_future(move || {
        let token = token.read().clone();
        let mut combineds = combineds;
        let mut sources = sources;
        let mut error = error;
        async move {
            let t = token.as_deref();
            match fetch_combineds(t).await {
                Ok(list) => combineds.set(list),
                Err(e) => error.set(e),
            }
            match fetch_sources(t).await {
                Ok(list) => sources.set(list),
                Err(e) => error.set(e),
            }
        }
    });

    // 打开新建弹窗
    let open_create = move |_| {
        form.set(FormState {
            open: true,
            edit_id: None,
            name: String::new(),
            checked: Vec::new(),
        });
    };

    // 打开编辑弹窗（预填名字与成员）
    let open_edit = move |id: i64| {
        let c = combineds.read().iter().find(|c| c.id == id).cloned();
        if let Some(c) = c {
            form.set(FormState {
                open: true,
                edit_id: Some(id),
                name: c.name,
                checked: c.source_ids,
            });
        }
    };

    // 勾选切换
    let toggle_member = move |sid: i64| {
        let mut f = form.read().clone();
        if let Some(pos) = f.checked.iter().position(|x| *x == sid) {
            f.checked.remove(pos);
        } else {
            f.checked.push(sid);
        }
        form.set(f);
    };

    // 保存（新建或编辑）
    let save = move |_| {
        let f = form.read().clone();
        if f.name.is_empty() {
            error.set("组合名称不能为空".into());
            return;
        }
        let token = token.read().clone();
        let body = serde_json::json!({
            "name": f.name,
            "source_ids": f.checked,
        })
        .to_string();
        let mut form = form.clone();
        let mut combineds = combineds.clone();
        let mut error = error.clone();
        let mut saving = saving.clone();
        let mut toasts = toasts.clone();
        saving.set(true);
        spawn(async move {
            let result = match f.edit_id {
                Some(id) => {
                    request("PUT", &format!("/admin/combineds/{}", id), Some(body), token.as_deref()).await
                }
                None => {
                    request("POST", "/admin/combineds", Some(body), token.as_deref()).await
                }
            };
            match result {
                Ok(_) => match fetch_combineds(token.as_deref()).await {
                    Ok(list) => {
                        combineds.set(list);
                        form.set(FormState::default());
                        error.set(String::new());
                        push_toast(toasts, ToastKind::Success, "组合订阅已保存");
                    }
                    Err(e) => error.set(e),
                },
                Err(e) => error.set(format!("保存失败: {e}")),
            }
            saving.set(false);
        });
    };

    // 删除确认
    let mut ask_delete = move |id: i64| {
        let name = combineds
            .read()
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.name.clone())
            .unwrap_or_default();
        pending_id.set(Some(id));
        confirm.set(ConfirmState {
            open: true,
            title: "删除组合订阅".into(),
            message: format!("确定删除「{}」？此操作不可撤销。", name),
            confirm_text: "删除".into(),
            danger: true,
        });
    };
    let on_confirm_delete = use_callback(move |_: ()| {
        confirm.set(ConfirmState::default());
        if let Some(id) = pending_id() {
            let token = token.read().clone();
            let mut combineds = combineds.clone();
            let mut error = error.clone();
            let mut toasts = toasts.clone();
            spawn(async move {
                match request("DELETE", &format!("/admin/combineds/{id}"), None, token.as_deref()).await {
                    Ok(_) => match fetch_combineds(token.as_deref()).await {
                        Ok(list) => {
                            combineds.set(list);
                            push_toast(toasts, ToastKind::Success, "已删除");
                        }
                        Err(e) => error.set(e),
                    },
                    Err(e) => push_toast(toasts, ToastKind::Error, format!("删除失败: {e}")),
                }
            });
        }
    });

    // 链接复制（label 门控反馈，与 config 页一致）
    let copy_click = move |label: &str, link: String| {
        let label = label.to_string();
        let mut copied = copied.clone();
        let toasts = toasts.clone();
        spawn(async move {
            match copy_text(link).await {
                Ok(()) => {
                    copied.set(Some(label.clone()));
                    push_toast(toasts, ToastKind::Success, "已复制到剪贴板");
                    let mut copied2 = copied.clone();
                    schedule_timeout(2000, move || {
                        if *copied2.read() == Some(label.clone()) {
                            copied2.set(None);
                        }
                    });
                }
                Err(e) => push_toast(toasts, ToastKind::Error, format!("复制失败: {e}")),
            }
        });
    };

    // 弹窗内成员复选框行（预渲染）
    let member_rows: Vec<Element> = sources
        .read()
        .iter()
        .map(|s| {
            let sid = s.id;
            let name = s.name.clone();
            let kind = s.kind.clone();
            let checked = form.read().checked.contains(&sid);
            rsx! {
                label { class: "member-row",
                    input {
                        r#type: "checkbox",
                        checked,
                        onchange: move |_| toggle_member(sid),
                    }
                    span { "{name}" }
                    span { class: format!("badge {}", if kind == "single" { "info" } else { "off" }),
                        if kind == "single" { "单条" } else { "远程" }
                    }
                    if !s.enabled {
                        span { class: "badge off", "停用" }
                    }
                }
            }
        })
        .collect();

    // 组合行（预渲染）
    let rows: Vec<Element> = combineds
        .read()
        .iter()
        .map(|c| {
            let id = c.id;
            let name = c.name.clone();
            let count = c.source_ids.len();
            let base = web_sys::window()
                .and_then(|w| w.location().origin().ok())
                .unwrap_or_default();
            let is_copied = copied.read().as_deref().map(|x| x == name).unwrap_or(false);
            rsx! {
                div { class: "combined-row",
                    div { class: "combined-info",
                        span { class: "combined-name", "{name}" }
                        span { class: "badge on", "{count} 个成员" }
                    }
                    div { class: "combined-links",
                        for fmt in ["clash", "v2ray", "singbox"] {
                            let link = format!("{}/subscribe/{}?format={}", base, name, fmt);
                            let link = link.clone();
                            button {
                                class: format!("btn btn-ghost btn-sm{}", if is_copied { " checked" } else { "" }),
                                onclick: move |_| copy_click(&name, link.clone()),
                                "{fmt}"
                            }
                        }
                    }
                    div { class: "actions",
                        button { class: "btn btn-ghost btn-sm", onclick: move |_| open_edit(id), {icon("config", 13)} "编辑" }
                        button { class: "btn btn-danger btn-sm", onclick: move |_| ask_delete(id), {icon("trash", 13)} "删除" }
                    }
                }
            }
        })
        .collect();

    let form_name = form.read().name.clone();
    let form_checked = form.read().checked.clone();
    rsx! {
        div { class: "page-head",
            h1 { class: "page-title", "组合订阅" }
            button { class: "btn btn-primary", onclick: open_create, {icon("plus", 14)} "新建组合" }
        }
        if !error.read().is_empty() {
            p { class: "error-text", "{error}" }
        }
        div { class: "card",
            if rows.is_empty() {
                div { class: "empty",
                    {icon("combineds", 36)}
                    span { class: "empty-title", "暂无组合订阅" }
                    span { class: "empty-hint", "新建组合并从订阅源中勾选成员，生成独立订阅链接" }
                }
            } else {
                {rows.into_iter()}
            }
        }
        // 新建/编辑弹窗
        if form.read().open {
            div { class: "modal-overlay", onclick: move |_| form.set(FormState::default()),
                div { class: "modal", onclick: move |e| e.stop_propagation(),
                    h3 { class: "modal-title", if form.read().edit_id.is_some() { "编辑组合" } else { "新建组合" } }
                    div { class: "field",
                        label { "组合名称（字母、数字、-、_）" }
                        input {
                            class: "mono",
                            placeholder: "例如：home",
                            value: form_name,
                            oninput: move |e| { let mut f = form.read().clone(); f.name = e.value(); form.set(f); },
                        }
                    }
                    p { class: "subtle", "选择包含的订阅源（可多选）" }
                    if member_rows.is_empty() {
                        div { class: "empty", span { class: "empty-hint", "暂无订阅源，请先到「订阅源」页添加" } }
                    } else {
                        {member_rows.into_iter()}
                    }
                    div { class: "modal-actions",
                        button { class: "btn btn-ghost", onclick: move |_| form.set(FormState::default()), "取消" }
                        button { class: "btn btn-primary", onclick: save, disabled: *saving.read(),
                            if *saving.read() { Spinner { size: 14 } } else { "保存" }
                        }
                    }
                }
            }
        }
        ConfirmDialog { state: confirm, on_confirm: on_confirm_delete }
    }
}
```

注意：
- 弹窗内 input 的 `value: form_name` 是渲染时快照，`oninput` 更新 `form` 信号（受控回显：每次渲染重建快照，信号更新触发重渲染，回显正常）
- `for fmt in ["clash", "v2ray", "singbox"]` 在 rsx 内是数组迭代（元素形式，非嵌套 rsx!），允许
- 复选框 `checked` 用 `form.read().checked.contains(&sid)` 快照 + onchange 切换信号

- [ ] **Step 5: index.html 样式**

`crates/server/web/index.html` 追加（`.link-row` 规则附近）：

```css
    /* ========== 组合订阅页 ========== */
    .combined-row { display: flex; align-items: center; gap: 12px; padding: 10px 0; border-bottom: 1px solid var(--border); }
    .combined-row:last-child { border-bottom: none; }
    .combined-info { display: flex; align-items: center; gap: 8px; min-width: 0; }
    .combined-name { font-family: var(--font-mono); font-size: 13.5px; font-weight: 500; white-space: nowrap; }
    .combined-links { display: flex; gap: 6px; margin-left: auto; }
    .member-row { display: flex; align-items: center; gap: 8px; padding: 6px 0; font-size: 13.5px; cursor: pointer; }
    .member-row input { accent-color: var(--accent); }
    .member-row .badge { margin-left: 2px; }
    .member-row .badge.off { margin-left: auto; }
```

- [ ] **Step 6: 构建验证 + 提交**

Run: `cd crates/server/web && dx build --web --release` → 成功。

```bash
git add -A
git commit -m "feat(web): combined subscriptions page with member picker and links
```
（提交信息结尾加 `Co-Authored-By: Claude <noreply@anthropic.com>`）

---

### Task 5: 前端——预览选择器 + 配置页精简

**Files:**
- Modify: `crates/server/web/src/components/preview.rs`
- Modify: `crates/server/web/src/components/config.rs`
- Modify: `crates/server/web/src/components/overview.rs`（若引用 ConfigDto 字段——不引用则跳过）

**Interfaces:**
- Consumes: `fetch_combineds`（Task 4）、`request`（api.rs）
- Produces: 预览页组合选择器（「全部源」+ 各组合）；配置页只留 Token 卡片

- [ ] **Step 1: preview.rs 组合选择器**

`crates/server/web/src/components/preview.rs`：

1. `use crate::components::combineds::fetch_combineds;`（组合列表）
2. 状态：`let combineds = use_signal(Vec::<CombinedDto>::new);` + `let mut selected = use_signal(|| None::<String>);`（None = 全部源）
3. `use_future`：并行加载组合列表（fetch_combineds）与初始预览
4. 抽取 `load_preview` 逻辑为闭包（selected 变化时带 `?combined=` 参数）：

```rust
    let mut load_preview = move |selected: Option<String>| {
        let token = token.read().clone();
        let mut data = data.clone();
        let mut loading = loading.clone();
        let mut error = error.clone();
        spawn(async move {
            loading.set(true);
            error.set(String::new());
            let path = match &selected {
                Some(name) => format!("/admin/preview?combined={}", name),
                None => "/admin/preview".to_string(),
            };
            match request("GET", &path, None, token.as_deref()).await {
                Ok(body) => match serde_json::from_str::<PreviewResp>(&body) {
                    Ok(r) => data.set(Some(r)),
                    Err(e) => error.set(format!("解析失败: {}", e)),
                },
                Err(e) => error.set(e.to_string()),
            }
            loading.set(false);
        });
    };
```

5. 页头加选择器（page-head 内、标题后）：

```rust
            select {
                class: "preview-filter",
                value: selected.read().clone().unwrap_or_default(),
                onchange: move |e| {
                    let v = e.value();
                    let v = if v.is_empty() { None } else { Some(v) };
                    selected.set(v.clone());
                    load_preview(v);
                },
                option { value: "", "全部源" }
                for c in combineds.read().iter() {
                    let name = c.name.clone();
                    option { value: name.clone(), selected: selected.read().as_deref() == Some(name.as_str()), "{name}" }
                }
            }
```

6. `use_future` 初始加载改为调用 `load_preview(None)`；`reload` 按钮改用 `load_preview(selected.read().clone())`。

7. index.html 加 `.preview-filter` 样式（对齐 `.field select` 基础样式）：

```css
    .preview-filter {
      background: var(--card); border: 1px solid var(--border); border-radius: 6px;
      padding: 6px 10px; font-size: 13px; color: var(--text); font-family: inherit;
    }
```

- [ ] **Step 2: config.rs 精简**

`crates/server/web/src/components/config.rs`：

1. `ConfigDto` 移除 `combined_name`、`subscribe_url`（只留 `admin_token`）
2. 移除：`new_name` 信号、`save_name` 闭包、组合订阅卡片、`links`/`link_rows`/`base_url`（链接归组合页）、`copy_click`（若不再被引用）
3. 保留：admin token 行（掩码/显示隐藏/轮换）、`ask_rotate`/`on_confirm_rotate`/`rotate`（admin only）
4. `use crate::components::copy_text;` 若 copy_click 移除后不再使用则删除 import；`Rc`、`JsFuture` 等 unused import 一并清理
5. 若 `toasts`/`push_toast` 仍被 rotate 使用则保留

- [ ] **Step 3: 构建验证 + 提交**

Run: `cd crates/server/web && dx build --web --release` → 成功。

```bash
git add -A
git commit -m "feat(web): preview combined filter, slim config page to tokens only
```
（提交信息结尾加 `Co-Authored-By: Claude <noreply@anthropic.com>`）

---

### Task 6: 文档与冒烟脚本

**Files:**
- Modify: `README.md`
- Modify: `CLAUDE.md`
- Modify: `scripts/smoke.sh`

- [ ] **Step 1: README.md 更新**

1. 功能段追加：`- 多个组合订阅：每组合从源中勾选成员（多对多），独立命名订阅链接（/subscribe/{name}），组合订阅管理在侧边栏「组合订阅」页`
2. 管理接口表加：

```markdown
| GET/POST | /admin/combineds | 组合订阅列表 / 创建（`source_ids` 成员源数组） |
| PUT/DELETE | /admin/combineds/{id} | 更新（名字/成员全量替换）/ 删除 |
```

3. 订阅接口段改为：`GET /subscribe/{name}?format=clash|v2ray|singbox`——`{name}` 为组合订阅名（`/admin/combineds` 中定义），无鉴权；名字不匹配 404；组合无成员时输出空配置（200）；全部成员源失败返回 502 附明细
4. 管理接口表 config 行说明改为：`获取配置 / 轮换 admin token`
5. 配置页描述更新（无组合名/链接，仅 token）

- [ ] **Step 2: CLAUDE.md 更新**

架构段路由描述改为：

```markdown
- `crates/server`：axum 服务。SQLite（sqlx）持久化 sources/settings/combined_subs/combined_sources。路由：`/subscribe/{name}`（组合订阅输出，按组合成员拉取，无鉴权）、`/admin/*`（源 CRUD/预览/配置/组合订阅 CRUD，Bearer 鉴权）；`/healthz`；SPA 静态托管 + 前端路由回退。
```

- [ ] **Step 3: smoke.sh 更新**

`scripts/smoke.sh`：

1. 步骤 5 的 config 断言移除 `combined_name`/`subscribe_url`（只断言 `admin_token`）。
2. 步骤 6 改为：创建源后**先创建组合并勾选**，再订阅：

```bash
# ---- 6. 加源 + 组合订阅 → 组合订阅输出 ----
step "6/9 加源与组合订阅 → /subscribe/{name}"
created="$(curl -sf -X POST "http://127.0.0.1:$SERVER_PORT/admin/sources" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  -d "{\"url\":\"http://127.0.0.1:$FIXTURE_PORT/sub.txt\",\"name\":\"fixture\"}")"
SRC_ID="$(python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])' <<<"$created")"

created_single="$(curl -sf -X POST "http://127.0.0.1:$SERVER_PORT/admin/sources" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"url":"ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#single-node","name":"single","kind":"single"}')"

# 组合勾选两个源
combined="$(curl -sf -X POST "http://127.0.0.1:$SERVER_PORT/admin/combineds" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  -d "{\"name\":\"merged\",\"source_ids\":[$SRC_ID]}")"
python3 -c 'import json,sys; d=json.load(sys.stdin); assert d["name"]=="merged"; assert d["source_ids"]==[int(sys.argv[1])], d; print("combined id=%d"%d["id"])' <<<"$combined" "$SRC_ID"

clash_out="$(curl -sf "http://127.0.0.1:$SERVER_PORT/subscribe/merged?format=clash")"
grep -q "fixture-node" <<<"$clash_out" || fail "/subscribe/merged 未输出 fixture-node"
grep -q "proxies:" <<<"$clash_out" || fail "/subscribe/merged 未输出 proxies 段"
printf 'GET /subscribe/merged?format=clash → 200 OK, 含 fixture-node\n'
```

3. 步骤 7（错误组合名 404）不变。
4. 文件头注释步骤 5-7 描述同步更新。

- [ ] **Step 4: 全量验证**

Run: `make smoke` → 全部通过。

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "docs: multi combined subscriptions in README/CLAUDE.md and smoke
```
（提交信息结尾加 `Co-Authored-By: Claude <noreply@anthropic.com>`）

---

### Task 7: 最终全量验证

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

Expected: 全部通过无警告。

- [ ] **Step 2: 浏览器人工核对清单**

1. 侧边栏出现「组合订阅」导航项
2. 组合订阅页：新建组合（名字校验）、勾选成员（单条/远程混合）、链接复制（三种格式）、编辑改名、删除确认
3. 配置页：无组合卡片，只有 Token 卡片
4. 预览页：组合选择器切换（全部源 / 各组合），节点数与成员匹配
5. `/subscribe/{name}` 无 token 参数，curl 200 且只含成员节点
