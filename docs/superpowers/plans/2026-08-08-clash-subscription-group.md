# Clash 订阅组模式 + 默认配置管理实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `/subscribe/{name}?format=clash` 输出改为订阅组模式（proxy-providers 引用组合订阅聚合链接），并新增前端「Clash 配置」页管理模板（dns/rules/头部 YAML 文本）。

**Architecture:** proxy-core 新增 `serialize_clash_subscription(template, provider_key, provider_url)` 纯函数（serde_yaml_ng 解析模板 + 插入系统段 providers/groups）；server 恢复 settings 读写 + 新路由 clash-config（GET/PUT，PUT 校验 YAML）+ subscribe clash 分支改读模板拼 provider url；前端 DataStore 加第 5 单元 + 新页面 + 导航 tab 重排。

**Tech Stack:** serde_yaml_ng 0.10 / axum 0.8 / sqlx 0.9；dioxus 0.8.0-alpha.1；ui-check.py（CDP）。

## Global Constraints

- 每次代码修改后必须按序通过：`cargo upgrade -i` → `cargo fmt --all` → `cargo clippy --workspace` → `cargo test --workspace`（CLAUDE.md 强制）
- Rust edition 2024，rust-version 1.97
- web crate 独立于 workspace（不进 members），仅由 `dx build --web --release --debug-symbols false` 构建，必须 0 警告
- dioxus 0.8 坑清单：rsx if/else 分支内不嵌套 `rsx!` 宏；use_effect 只在挂载 + 被读信号变化时重跑；Signal::set 需 let mut
- 叶子索引：`0=本地 1=远程 2=组合 3=Clash 配置 4=配置`；默认 tab=0
- provider url：`{scheme}://{请求Host}/subscribe/{name}?format=v2ray`（scheme 取 X-Forwarded-Proto 或默认 http；Host 缺失 → 400）
- 系统段 = proxy-providers + proxy-groups（解析合并，覆盖模板同键）；头部/dns/rules 来自模板
- v2ray/singbox 分支照旧（解析输出）
- 默认模板 = `mixed-port: 7890\nallow-lan: false\nmode: rule\nlog-level: info\n\nrules:\n  - MATCH,🚀 节点选择\n`
- spec：docs/superpowers/specs/2026-08-08-clash-subscription-group-design.md

---

### Task 1: proxy-core — serialize_clash_subscription 纯函数

**Files:**
- Modify: `crates/proxy-core/src/formats/clash.rs`
- Modify: `crates/proxy-core/src/error.rs`（SerializeError 加 InvalidTemplate 变体）
- Test: `crates/proxy-core/tests/formats.rs`

**Interfaces:**
- Consumes: 现状（Task 0 基线）
- Produces（Task 2 依赖）:
  - `pub fn serialize_clash_subscription(template: &str, provider_key: &str, provider_url: &str) -> Result<String, SerializeError>`
  - `SerializeError::InvalidTemplate(String)` 新变体

- [x] **Step 1: 写失败测试（tests/formats.rs 追加）**

```rust
use proxy_core::formats::clash::serialize_clash_subscription;

#[test]
fn clash_subscription_default_template() {
    let tpl = "mixed-port: 7890\nallow-lan: false\nmode: rule\nlog-level: info\n";
    let out = serialize_clash_subscription(tpl, "home", "http://x/subscribe/home?format=v2ray").unwrap();
    assert!(out.contains("mixed-port: 7890"), "头部保留");
    assert!(out.contains("proxy-providers:"));
    assert!(out.contains("home:"));
    assert!(out.contains("url: http://x/subscribe/home?format=v2ray"));
    assert!(out.contains("proxy-groups:"));
    assert!(out.contains("use:"));
    assert!(out.contains("- home"));
    // 输出必须是合法 YAML 且 providers 恰好一个
    let v: serde_yaml_ng::Value = serde_yaml_ng::from_str(&out).unwrap();
    let prov = v["proxy-providers"].as_mapping().unwrap();
    assert_eq!(prov.len(), 1);
    assert!(prov.contains_key(&serde_yaml_ng::Value::String("home".into())));
}

#[test]
fn clash_subscription_keeps_custom_sections() {
    let tpl = "mode: rule\ndns:\n  enable: true\n  nameserver:\n    - 1.1.1.1\nrules:\n  - DOMAIN-SUFFIX,google.com,🚀 节点选择\n";
    let out = serialize_clash_subscription(tpl, "home", "http://x/sub").unwrap();
    assert!(out.contains("dns:"));
    assert!(out.contains("enable: true"));
    assert!(out.contains("1.1.1.1"));
    assert!(out.contains("DOMAIN-SUFFIX,google.com"));
}

#[test]
fn clash_subscription_system_sections_override() {
    let tpl = "proxy-providers:\n  evil: {type: file, path: ./x}\nproxy-groups:\n  - name: evil\n";
    let out = serialize_clash_subscription(tpl, "home", "http://x/sub").unwrap();
    let v: serde_yaml_ng::Value = serde_yaml_ng::from_str(&out).unwrap();
    let prov = v["proxy-providers"].as_mapping().unwrap();
    assert_eq!(prov.len(), 1, "模板 providers 必须被系统段覆盖");
    assert!(prov.contains_key(&serde_yaml_ng::Value::String("home".into())));
    assert!(!out.contains("evil"), "模板 providers/groups 不得残留");
}

#[test]
fn clash_subscription_invalid_template() {
    assert!(serialize_clash_subscription(": : :", "home", "http://x").is_err());
    assert!(serialize_clash_subscription("", "home", "http://x").is_ok(), "空模板视为空映射，合法");
}
```

- [x] **Step 2: 跑测试确认失败**

Run: `cargo test -p proxy-core --test formats clash_subscription`
Expected: 编译错误（函数不存在）

- [x] **Step 3: 实现**

error.rs 加变体：

```rust
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SerializeError {
    #[error("unsupported protocol for this format: {0}")]
    UnsupportedProtocol(&'static str),
    #[error("missing required field: {0}")]
    MissingField(&'static str),
    #[error("invalid clash template: {0}")]
    InvalidTemplate(String),
}
```

clash.rs：

```rust
/// 订阅组模式输出：模板（头部/dns/rules 等用户自定义段）+ 系统自动追加的
/// proxy-providers / proxy-groups 两段（解析合并，覆盖模板同键）。
/// provider 引用 sub-merge 自己的组合订阅聚合链接（v2ray base64 订阅，mihomo http provider 可拉取）。
pub fn serialize_clash_subscription(
    template: &str,
    provider_key: &str,
    provider_url: &str,
) -> Result<String, SerializeError> {
    let mut v: serde_yaml_ng::Value = serde_yaml_ng::from_str(template)
        .map_err(|e| SerializeError::InvalidTemplate(e.to_string()))?;
    if !v.is_mapping() {
        return Err(SerializeError::InvalidTemplate("root must be a mapping".into()));
    }
    // 系统段：proxy-providers + proxy-groups（覆盖模板同键）
    let providers = serde_yaml_ng::to_value(serde_json::json!({
        provider_key: {
            "type": "http",
            "url": provider_url,
            "interval": 3600,
            "path": format!("./providers/{}.yaml", provider_key),
            "health-check": {
                "enable": true,
                "url": "http://www.gstatic.com/generate_204",
                "interval": 300,
            },
        }
    }))
    .map_err(|e| SerializeError::InvalidTemplate(e.to_string()))?;
    let groups = serde_yaml_ng::to_value(serde_json::json!([
        {
            "name": "🚀 节点选择",
            "type": "select",
            "use": [provider_key],
            "proxies": ["DIRECT"],
        },
        {
            "name": "♻️ 自动选择",
            "type": "url-test",
            "url": "http://www.gstatic.com/generate_204",
            "interval": 300,
            "use": [provider_key],
        },
    ]))
    .map_err(|e| SerializeError::InvalidTemplate(e.to_string()))?;
    v.as_mapping_mut().unwrap().insert("proxy-providers".into(), providers);
    v.as_mapping_mut().unwrap().insert("proxy-groups".into(), groups);
    serde_yaml_ng::to_string(&v).map_err(|e| SerializeError::InvalidTemplate(e.to_string()))
}
```

注意：serde_json 的 `json!` 宏在 proxy-core 已有依赖（serde_json = "1"）。`serde_yaml_ng::to_value(impl Serialize)` 接受 serde_json::Value。若 `serde_yaml_ng::Value::String` 的 as_mapping 等 API 细节有出入，按实际 API 适配（serde_yaml_ng 的 Value 与 serde_json::Value 结构类似：as_mapping_mut 存在）。

- [x] **Step 4: 跑测试确认通过**

Run: `cargo test -p proxy-core --test formats clash_subscription`
Expected: 全 PASS

- [x] **Step 5: 门禁 + commit**

Run: `cargo fmt --all && cargo clippy --workspace && cargo test --workspace`

```bash
git add crates/proxy-core/src/formats/clash.rs crates/proxy-core/src/error.rs crates/proxy-core/tests/formats.rs
git commit -m "feat(proxy-core): serialize_clash_subscription 订阅组模式序列化（模板 + 系统段 providers/groups）"
```

---

### Task 2: 后端 — settings 读写 + clash-config API + subscribe 改造

**Files:**
- Modify: `crates/server/src/db.rs`（恢复 get_setting/set_setting）
- Create: `crates/server/src/routes/clash_config.rs`
- Modify: `crates/server/src/routes/mod.rs`（挂载）
- Modify: `crates/server/src/routes/subscribe.rs`（clash 分支改造）
- Test: `crates/server/tests/api_test.rs`

**Interfaces:**
- Consumes: Task 1 的 `serialize_clash_subscription`；现状 require_admin/build_router
- Produces:
  - `GET /admin/clash-config` → `{"template": "..."}`；`PUT /admin/clash-config`（body `{"template": "..."}`，YAML 非法 400）
  - `default_template() -> String`（pub(crate)，clash_config.rs）
  - subscribe clash 分支输出订阅组模式；Host 缺失 400；X-Forwarded-Proto 支持

- [x] **Step 1: db.rs 恢复 settings 读写**

```rust
pub async fn get_setting(pool: &SqlitePool, key: &str) -> Result<Option<String>> {
    let row = sqlx::query("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.get::<String, _>(0)))
}

pub async fn set_setting(pool: &SqlitePool, key: &str, value: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}
```

（`use sqlx::Row;` db.rs 已有。）

- [x] **Step 2: 写失败测试（api_test.rs 追加）**

```rust
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
    let (s, _) = http(&app, "PUT", "/admin/clash-config",
        Some(json!({"template": ": : :"}).to_string()), Some(&admin)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // PUT 合法模板 → 保存成功，回读一致
    let custom = "mode: rule\ndns:\n  enable: true\n  nameserver:\n    - 1.1.1.1\nrules:\n  - MATCH,🚀 节点选择\n";
    let (s, v) = http(&app, "PUT", "/admin/clash-config",
        Some(json!({"template": custom}).to_string()), Some(&admin)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["template"].as_str().unwrap(), custom);
    let (s, v) = http(&app, "GET", "/admin/clash-config", None, Some(&admin)).await;
    assert_eq!(v["template"].as_str().unwrap(), custom, "保存后回读一致");

    // 无鉴权 → 401
    let (s, _) = http(&app, "GET", "/admin/clash-config", None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // 建组合 + 订阅 → clash 输出为订阅组模式（含 providers + 自定义 dns + use 引用）
    sqlx::query("INSERT INTO sources (url, name, kind, enabled, created_at) VALUES ('ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#A', 's1', 'single', 1, 'now')")
        .execute(&pool).await.unwrap();
    let src_id: i64 = sqlx::query_scalar("SELECT id FROM sources WHERE name = 's1'").fetch_one(&pool).await.unwrap();
    sqlx::query("INSERT INTO combined_subs (name, created_at) VALUES ('grp', 'now')").execute(&pool).await.unwrap();
    let cid: i64 = sqlx::query_scalar("SELECT id FROM combined_subs WHERE name = 'grp'").fetch_one(&pool).await.unwrap();
    sqlx::query("INSERT INTO combined_sources (combined_id, source_id) VALUES (?, ?)").bind(cid).bind(src_id).execute(&pool).await.unwrap();

    let (s, body) = http_raw(&app, "GET", "/subscribe/grp?format=clash", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.contains("proxy-providers:"), "订阅组模式输出");
    assert!(body.contains("grp:"), "provider key = 组合名");
    assert!(body.contains("url: http://example.com/subscribe/grp?format=v2ray"), "provider url 拼请求 Host");
    assert!(body.contains("use:"));
    assert!(body.contains("- grp"));
    assert!(body.contains("dns:"), "自定义模板段保留");
    assert!(!body.contains("proxies:\n  - name:"), "不再输出解析节点");
}
```

需要 `http_raw` helper（返回原始 body 字符串而非 JSON Value）——api_test 现有 `http` 返回 Value；加一个变体：

```rust
async fn http_raw(app: &axum::Router, method: &str, uri: &str, body: Option<String>, token: Option<&str>)
    -> (axum::http::StatusCode, String) {
    let mut b = axum::http::Request::builder().method(method).uri(uri)
        .header("host", "example.com"); // 固定 Host 供 provider url 断言
    if let Some(t) = token { b = b.header("authorization", format!("Bearer {t}")); }
    let req = match body {
        Some(s) => b.header("content-type", "application/json").body(axum::body::Body::from(s)).unwrap(),
        None => b.body(axum::body::Body::empty()).unwrap(),
    };
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    (status, String::from_utf8_lossy(&bytes).to_string())
}
```

Host 缺失 → 400 测试：

```rust
#[tokio::test]
async fn subscribe_clash_without_host_returns_400() {
    let tmp = fresh_tmp("clash-nohost");
    let pool = test_pool(&tmp).await;
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg).await;
    // 建空组合
    sqlx::query("INSERT INTO combined_subs (name, created_at) VALUES ('grp', 'now')").execute(&pool).await.unwrap();
    // 无 Host header 的请求
    let resp = app.clone().oneshot(
        axum::http::Request::builder().uri("/subscribe/grp?format=clash")
            .body(axum::body::Body::empty()).unwrap(),
    ).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
```

（`http`/`http_raw` 带固定 host 的请求会覆盖 axum 自动 Host——注意现有 `http` helper 未设 host，axum 的 oneshot 对无 Host 的请求…… axum 0.8 中 Host header 缺失时 `headers.get(HOST)` 返回 None → 400。但现有 subscribe 测试（subscribe_returns_subscription 等）用旧 http helper 无 Host——clash 分支改造后这些测试会 400！**必须检查**：既有测试 `subscribe_returns_subscription` 请求 /subscribe/merged?format=clash 无 Host → 改造后 400 → 测试失败。处理：既有测试的 http helper 统一加默认 Host（`"example.com"`），或仅 clash 相关测试用 http_raw。**方案：在 http helper 里统一加 `.header("host", "example.com")`**——对所有请求无害（axum 不校验 Host 内容），并让既有 clash 订阅测试继续工作。v2ray/singbox 测试不受 Host 影响。）

- [x] **Step 3: 跑测试确认失败**

Run: `cargo test --test api_test clash_config_get_put_and_subscription_output`
Expected: FAIL（404 路由未注册 / clash 输出仍是解析节点）

- [x] **Step 4: 实现 clash_config.rs + 挂载**

```rust
// crates/server/src/routes/clash_config.rs
use crate::auth::require_admin;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

pub(crate) const TEMPLATE_KEY: &str = "clash_template";

pub(crate) fn default_template() -> String {
    "mixed-port: 7890\nallow-lan: false\nmode: rule\nlog-level: info\n\nrules:\n  - MATCH,🚀 节点选择\n".to_string()
}

pub fn router() -> Router<AppState> {
    Router::new().route("/admin/clash-config", get(get_config).put(put_config))
}

#[derive(Serialize)]
pub struct ClashConfigDto { pub template: String }

#[derive(Deserialize)]
pub struct UpdateClashConfig { pub template: String }

async fn get_config(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<ClashConfigDto>, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    let template = crate::db::get_setting(&state.pool, TEMPLATE_KEY)
        .await?
        .unwrap_or_else(default_template);
    Ok(Json(ClashConfigDto { template }))
}

async fn put_config(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: Result<Json<UpdateClashConfig>, JsonRejection>,
) -> Result<Json<ClashConfigDto>, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    let Json(b) = body.map_err(ApiError::from)?;
    // YAML 合法性校验（根必须是 mapping——serialize_clash_subscription 会再次校验）
    let v: serde_yaml_ng::Value = serde_yaml_ng::from_str(&b.template)
        .map_err(|e| ApiError::bad_request(format!("invalid YAML: {e}")))?;
    if !v.is_mapping() {
        return Err(ApiError::bad_request("template must be a YAML mapping"));
    }
    crate::db::set_setting(&state.pool, TEMPLATE_KEY, &b.template).await?;
    Ok(Json(ClashConfigDto { template: b.template }))
}
```

mod.rs 挂载：`pub mod clash_config;` + `.merge(clash_config::router())`。

- [x] **Step 5: subscribe.rs clash 分支改造**

```rust
// subscribe_handler 中 format 解析后：
let format = match &q.format { ... };

// clash 分支：订阅组模式（不再解析节点输出）
if format == OutputFormat::Clash {
    // 组合名作为 provider key
    let scheme = state
        .cfg
        .clone(); // 不需要 cfg——从 headers 取
    // 实际实现：
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .filter(|s| s == &"https")
        .map(|_| "https")
        .unwrap_or("http");
    let host = headers
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| ApiError::bad_request("missing Host header"))?;
    let provider_url = format!("{scheme}://{host}/subscribe/{name}?format=v2ray");
    let template = crate::db::get_setting(&state.pool, crate::routes::clash_config::TEMPLATE_KEY)
        .await?
        .unwrap_or_else(crate::routes::clash_config::default_template);
    let body = proxy_core::formats::clash::serialize_clash_subscription(&template, &name, &provider_url)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    return Ok(Response::builder()
        .header("content-type", "application/x-yaml")
        .header("profile-update-interval", "24")
        .body(axum::body::Body::from(body))
        .unwrap());
}
// v2ray/singbox 分支照旧（fetch_and_merge + serialize_nodes）
```

注意：subscribe_handler 需要 headers 参数（现状没有——加 `headers: HeaderMap` 提取器）。原 handler 签名：`(State, Path, Query)`。加 `headers: axum::http::HeaderMap`。

- [x] **Step 6: 既有测试适配（http helper 加默认 Host）**

api_test.rs 的 `http` helper 与 `http_raw` 统一加 `.header("host", "example.com")`（既有 clash 订阅测试 subscribe_returns_subscription 等依赖——否则 Host 缺失 400 会破坏它们）。`setup_admin` 内部调用不受影响。

- [x] **Step 7: 跑测试确认通过**

Run: `cargo test --test api_test clash_config_get_put_and_subscription_output subscribe_clash_without_host_returns_400 subscribe_returns_subscription`
Expected: 全 PASS

- [x] **Step 8: 门禁 + commit**

Run: `cargo fmt --all && cargo clippy --workspace && cargo test --workspace`

```bash
git add crates/server/src/db.rs crates/server/src/routes/ crates/server/tests/api_test.rs
git commit -m "feat(routes): clash 输出改订阅组模式（模板 + providers）+ clash-config 配置 API"
```

---

### Task 3: 前端 — clash_config 单元 + 配置页 + 导航

**Files:**
- Modify: `crates/server/web/src/data.rs`（第 5 单元）
- Create: `crates/server/web/src/components/clash_config.rs`
- Modify: `crates/server/web/src/components/mod.rs`
- Modify: `crates/server/web/src/main.rs`（导航 tab 重排 + NavLeaf）
- Modify: `crates/server/web/src/components/icon.rs`（"clash" 图标）
- Modify: `crates/server/web/index.html`（textarea 样式）

**Interfaces:**
- Consumes: Task 2 的 `/admin/clash-config` API
- Produces: 叶子 tab=3「Clash 配置」页面（textarea 编辑 + 保存）；DataStore.ClashConfig 单元；required_units: 3→[ClashConfig]、4→[Config]

- [x] **Step 1: data.rs 加单元**

```rust
// UnitKey 加 ClashConfig 变体
pub enum UnitKey { Sources, Combineds, Config, ClashConfig }

// fetch_clash_config:
pub async fn fetch_clash_config(token: Option<&str>) -> Result<String, String> {
    let body = request("GET", "/admin/clash-config", None, token).await?;
    let dto: submerge_web_core::dto::ClashConfigDto = serde_json::from_str(&body).map_err(|e| format!("解析失败: {}", e))?;
    Ok(dto.template)
}

// DataStore struct 加 clash_config: Signal<CacheState<String>>
// provide/status_of/load（Loading 置位、stale、fetch 分支）
// required_units:
//   3 => &[UnitKey::ClashConfig],   // Clash 配置
//   _ => &[UnitKey::Config],        // 配置
```

web-core dto.rs 加：

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct ClashConfigDto {
    pub template: String,
}
```

（web-core 在 workspace 内——dto 加 struct + 解析测试随 cargo test 覆盖）

- [x] **Step 2: clash_config.rs 页面**

```rust
// crates/server/web/src/components/clash_config.rs
// Clash 默认配置页：YAML 模板编辑（头部/dns/rules）。proxy-providers/proxy-groups 由系统自动追加。
use crate::api::request;
use crate::components::icon::Spinner;
use crate::components::toast::{ToastKind, push_toast, use_toast};
use crate::data::{DataStore, UnitKey};
use dioxus::prelude::*;

#[component]
pub fn ClashConfig(token: Signal<Option<String>>) -> Element {
    let data = use_context::<DataStore>();
    let mut draft = use_signal(String::new);
    let mut inited = use_signal(|| false);
    let mut error = use_signal(String::new);
    let mut saving = use_signal(|| false);
    let toasts = use_toast();

    // 挂载时从缓存单元初始化草稿（use_effect 挂载 + 缓存变化时重跑，守卫防覆盖已编辑内容）
    let state = data.clash_config.read().clone();
    use_effect(move || {
        if !inited() {
            if let Some(t) = data.clash_config.read().data.clone() {
                draft.set(t);
                inited.set(true);
            }
        }
    });

    let mut save = move |_| {
        let t = draft.read().clone();
        if t.trim().is_empty() {
            error.set("模板不能为空".into());
            return;
        }
        let current = token.read().clone();
        let body = serde_json::json!({ "template": t }).to_string();
        let mut error = error.clone();
        let mut saving = saving.clone();
        let toasts = toasts.clone();
        saving.set(true);
        spawn(async move {
            match request("PUT", "/admin/clash-config", Some(body), current.as_deref()).await {
                Ok(_) => {
                    data.refresh(UnitKey::ClashConfig);
                    error.set(String::new());
                    push_toast(toasts, ToastKind::Success, "Clash 配置已保存");
                }
                Err(e) => error.set(format!("保存失败: {e}")),
            }
            saving.set(false);
        });
    };

    let page_error = if error.read().is_empty() {
        state.error.clone()
    } else {
        error.read().clone()
    };

    rsx! {
        div { class: "page-head",
            h1 { class: "page-title", "Clash 配置" }
            button { class: "btn btn-primary", onclick: save, disabled: *saving.read(),
                if *saving.read() { Spinner { size: 14 } } else { "保存" }
            }
        }
        if !page_error.is_empty() {
            p { class: "error-text", "{page_error}" }
        }
        div { class: "card",
            p { class: "subtle", "在此编辑 Clash 输出的默认配置（头部字段、dns、分流 rules 等）。proxy-providers 与 proxy-groups 由系统自动追加，无需（也不应）在此编写。" }
            textarea {
                class: "clash-template",
                rows: "24",
                placeholder: "mixed-port: 7890\nallow-lan: false\nmode: rule\nlog-level: info\n\nrules:\n  - MATCH,🚀 节点选择",
                value: draft,
                oninput: move |e| draft.set(e.value()),
            }
        }
    }
}
```

- [x] **Step 3: main.rs 导航 tab 重排 + icon + mod.rs + CSS**

main.rs：
- 叶子索引 3 = ClashConfig 叶子（NavLeaf name="clash" label="Clash 配置"），4 = 配置
- 祖先强制展开逻辑：`if i < 2 { insert("single") }` 不变（0/1 在单条订阅下）
- content match：
```rust
3 => rsx! { ClashConfig { token } },
_ => rsx! { Config { token } },
```
- use 引入 ClashConfig

icon.rs 加 "clash"（文件代码图标，Lucide file-code）：

```rust
"clash" => rsx! {
    svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
        path { d: "M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z" }
        path { d: "M14 2v4a2 2 0 0 0 2 2h4" }
        path { d: "m10 13-2 2 2 2" }
        path { d: "m14 17 2-2-2-2" }
    }
},
```

mod.rs：`pub mod clash_config;`

index.html（表单段追加）：

```css
.field textarea, textarea.clash-template {
  width: 100%; background: var(--card); border: 1px solid var(--border); border-radius: 6px;
  padding: 8px 10px; font-size: 13px; color: var(--text); font-family: var(--font-mono);
  line-height: 1.5; resize: vertical; box-sizing: border-box;
  transition: border-color .15s ease, box-shadow .15s ease;
}
textarea.clash-template:focus { outline: none; border-color: var(--accent); box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 18%, transparent); }
```

- [x] **Step 4: 构建验证 + 门禁 + commit**

Run: `cd crates/server/web && dx build --web --release --debug-symbols false`（0 警告）→ `cargo fmt --all && cargo clippy --workspace && cargo test --workspace`（web-core dto 新测试随跑）

```bash
git add crates/server/web/src/ crates/server/web/index.html crates/web-core/src/dto.rs
git commit -m "feat(web): Clash 配置页（模板编辑）+ 导航新增叶子 + clash_config 数据单元"
```

---

### Task 4: ui-check.py + 文档

**Files:**
- Modify: `scripts/ui-check.py`（导航文本匹配冲突修复 + clash_config 场景）
- Modify: `README.md`、`CLAUDE.md`

**Interfaces:**
- Consumes: Task 1-3 全部
- Produces: ui-check 既有场景在新导航下全 PASS + 新增 clash_config 场景；文档同步

- [x] **Step 1: 修复「配置」文本匹配冲突（关键跨任务坑）**

现状：ui-check 的 `nav_el`/`nav_loading`/`nav_active`/`click_nav` 用 `textContent.includes(label)` 匹配。新导航有「Clash 配置」与「配置」两个按钮——`includes('配置')` 会**先命中「Clash 配置」**（DOM 顺序 tab=3 在前）。既有场景（nav_preload 慢路径、config_password）点「配置」会点错。

修复：新增精确匹配 helper，所有导航相关断言改用它：

```python
def nav_button(ws, label):
    # 精确匹配按钮文本（trim 后全等）——避免「配置」误命中「Clash 配置」
    return ev(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.trim()==='%s')!==undefined" % label)

def nav_loading_exact(ws, label):
    return ev(ws, "!!(Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.trim()==='%s')?.querySelector('.spinner'))" % label)

def nav_active_exact(ws, label):
    return ev(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.trim()==='%s').classList.contains('active')" % label)

def click_nav_exact(ws, label):
    ev(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.trim()==='%s').click()" % label)
    time.sleep(0.3)
```

既有场景中所有 `nav_el/nav_loading/nav_active/click_nav` 对「本地订阅」「远程订阅」「组合订阅」「配置」的调用改精确版（「单条订阅」「订阅管理」分组按钮无歧义可保留 includes，但统一改精确更稳）。`scenario_config_password` 与 `nav_preload` 的「配置」调用必须改。

新增场景：

```python
def scenario_clash_config(ws):
    """Clash 配置页：编辑模板 → 保存 → 断言保存成功与回读。"""
    login(ws)
    click_nav_exact(ws, "Clash 配置")
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.trim()==='Clash 配置').classList.contains('active')"), "Clash 配置就绪")
    assert_true(wait_until(ws, "!!document.querySelector('textarea.clash-template')"), "模板编辑区出现")
    assert_true(ev(ws, "document.querySelector('textarea.clash-template').value.includes('mixed-port')"), "草稿初始化为默认模板")
    # 编辑 + 保存
    ev(ws, "(()=>{const t=document.querySelector('textarea.clash-template');const s=Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value').set;s.call(t, t.value + '\\n# ui-check 注释\\n');t.dispatchEvent(new Event('input',{bubbles:true}));})()")
    ev(ws, "Array.from(document.querySelectorAll('.page-head button')).find(b=>b.textContent.trim()==='保存').click()")
    assert_true(wait_until(ws, "document.body.innerText.includes('已保存')", timeout=10), "保存成功 toast")
    # 刷新页面 → 回读模板含编辑内容
    cmd(ws, "Page.reload")
    time.sleep(2.5)
    assert_true(wait_until(ws, "!!document.querySelector('textarea.clash-template')"), "刷新后配置页就绪")
    assert_true(ev(ws, "document.querySelector('textarea.clash-template').value.includes('# ui-check 注释')"), "保存内容回读")
    # 恢复默认模板（清掉 ui-check 注释，避免毒害后续订阅输出）
    ev(ws, "(()=>{const t=document.querySelector('textarea.clash-template');const s=Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value').set;s.call(t, t.value.replace('\\n# ui-check 注释\\n',''));t.dispatchEvent(new Event('input',{bubbles:true}));})()")
    ev(ws, "Array.from(document.querySelectorAll('.page-head button')).find(b=>b.textContent.trim()==='保存').click()")
    assert_true(wait_until(ws, "document.body.innerText.includes('已保存')", timeout=10), "恢复保存")
```

main() 的 scenarios dict 加 `"clash_config": scenario_clash_config`。场景顺序：clash_config 放在 config_password 之后（同域操作）。

- [x] **Step 2: 文档更新**

README：
- API 表加：
  `| GET/PUT | /admin/clash-config | 获取/更新 Clash 默认配置模板（YAML 文本，dns/分流 rules 等；providers/groups 由系统自动追加） |`
- clash 格式说明（「注意：V2Ray 格式的节点覆盖」段附近或 API 订阅接口段）更新：`format=clash` 输出订阅组模式（proxy-providers 引用组合订阅聚合链接，Clash 客户端直拉；节点由聚合订阅提供）

CLAUDE.md：
- 架构节 server 行路由描述加 clash-config；web 行 DataStore「四单元」→「五单元」

- [x] **Step 3: 实跑验证 + 门禁 + commit**

Run: chrome-headless-shell（/opt 已装）+ 起 server → `python3 scripts/ui-check.py nav_preload` 与 `python3 scripts/ui-check.py clash_config` 与 `config_password`（导航改动回归）→ 全 PASS
Run: `python3 -m py_compile scripts/ui-check.py` + `cargo fmt --all && cargo clippy --workspace && cargo test --workspace`

```bash
git add scripts/ui-check.py README.md CLAUDE.md
git commit -m "chore(ui-check): 导航精确匹配修复 + clash_config 场景；文档同步订阅组模式"
```

---

## 自审记录

**Spec 覆盖检查：**
- serialize_clash_subscription 纯函数（模板合并/覆盖/非法）→ Task 1
- settings 读写恢复 + clash-config API（GET/PUT + YAML 校验 400）→ Task 2
- subscribe clash 分支（模板 + provider url + Host 400 + X-Forwarded-Proto）→ Task 2
- 前端 clash_config 单元 + 页面 + 导航叶子（tab=3/4 重排）→ Task 3
- ui-check clash_config 场景 + 既有场景「配置」匹配冲突修复 → Task 4
- README/CLAUDE.md → Task 4
- v2ray/singbox 不变 → Task 2 Step 7 既有测试保持
- 空成员组合照常输出 → Task 2 测试含空组合 Host 400 用例（正常空组合输出在 subscribe_without_token_succeeds 既有测试覆盖——该测试无 Host！Task 2 Step 6 的 http helper 加默认 Host 后它继续通过）

**无占位符检查：** 各任务含完整代码；Task 1 的 serde_yaml_ng API 细节注明按实际适配。

**类型一致性：** `serialize_clash_subscription(template, provider_key, provider_url) -> Result<String, SerializeError>` Task 1 定义 Task 2 使用；`TEMPLATE_KEY`/`default_template` Task 2 定义（subscribe.rs 引用 `crate::routes::clash_config::` 路径）；`ClashConfigDto` web-core 定义（Task 3 Step 1）被 data.rs 的 fetch_clash_config 使用；`ClashConfig(token)` 组件 Task 3 定义被 main.rs 引用；ui-check 精确匹配 helper（nav_button/nav_loading_exact/nav_active_exact/click_nav_exact）Task 4 定义并被既有场景改造引用。
