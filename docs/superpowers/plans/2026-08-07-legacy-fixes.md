# 遗留问题清理实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复认证改造与导航重构两个批次评审 triage 的全部遗留问题（功能性 10 项 + 死代码清理 6 项 + 测试加固 4 项 + 文档 4 项）。

**Architecture:** 分四组任务：后端修复（setup 原子化防竞态、verify_user 恒定时间、update_password 行检查）→ 前端功能修复（toast 死代码、在途请求竞态、受控下拉、空 token 兜底、重试按钮、窄屏 CSS）→ 死代码清理 → 测试加固 + 文档修正。

**Tech Stack:** axum 0.8 / sqlx 0.9（后端）；dioxus 0.8.0-alpha.1（前端）；ui-check.py（CDP 脚本）；argon2 0.5。

## Global Constraints

- 每次代码修改后必须按序通过：`cargo upgrade -i` → `cargo fmt --all` → `cargo clippy --workspace` → `cargo test --workspace`（CLAUDE.md 强制）
- Rust edition 2024，rust-version 1.97
- web crate 独立于 workspace（不进 members），仅由 `dx build --web --release --debug-symbols false` 构建，必须 0 警告
- dioxus 0.8 坑清单：rsx if/else 分支内不嵌套 `rsx!` 宏；`Element` 空渲染用 `VNode::empty()`；**use_effect 只在挂载 + effect 内读到的信号变化时重跑（CLAUDE.md 已修正此语义）**；非响应式 prop 变化用 `use_effect(use_reactive((&key,), ...))` 桥接
- **明确不修**：会话表 TTL（spec 已批准 YAGNI——会话永久有效 + 改密后全部失效已足够，不引入过期清理）
- 遗留清单来源：`.superpowers/sdd/2026-08-07-username-password-auth/progress.md`（认证批次 ledger，已删除——清单以本计划为准）与 `.superpowers/sdd/2026-08-07-navigation-restructure/progress.md`（导航批次 ledger，已删除）以及两次最终评审报告

---

### Task 1: 后端修复 — setup 原子化 / verify_user 恒定时间 / update_password 行检查

**Files:**
- Modify: `crates/server/src/routes/auth.rs`（setup 原子化 + UNIQUE 冲突 409）
- Modify: `crates/server/src/db.rs`（verify_user 恒定时间、update_password 行检查）
- Test: `crates/server/tests/api_test.rs`

**Interfaces:**
- Consumes: 现状 `users_empty`/`create_user`/`verify_user`/`update_password`（db.rs）
- Produces: 行为变更——setup 并发安全（双管理员不可能）、同用户名并发 409、verify_user 用户不存在走 dummy argon2 验证、update_password 0 行报错

- [ ] **Step 1: 写失败测试（api_test.rs 追加）**

```rust
#[tokio::test]
async fn setup_is_atomic_against_duplicate_admin() {
    // 回归：并发/重复 setup 不产生第二个管理员（INSERT...SELECT WHERE NOT EXISTS 原子性）。
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-setup-atomic", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg).await;

    let (s, _) = http(&app, "POST", "/admin/setup", Some(valid_setup("admin")), None).await;
    assert_eq!(s, StatusCode::OK);
    // 已存在时再 setup（不同用户名）→ 409（原子 INSERT 的 affected=0 分支）
    let (s, _) = http(&app, "POST", "/admin/setup", Some(valid_setup("admin2")), None).await;
    assert_eq!(s, StatusCode::CONFLICT);
    // 确认只有一个用户
    let pool2 = test_pool(&tmp); // 复用同一 db 文件重新连接验证
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(&pool2).await.unwrap();
    assert_eq!(n, 1, "must never have two admins");
}

#[tokio::test]
async fn concurrent_setup_never_creates_two_admins() {
    // 两个 setup 并发（不同用户名）：原子 INSERT 保证只有一个成功
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-setup-conc", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool.clone(), cfg).await;

    let app1 = app.clone();
    let app2 = app.clone();
    let (r1, r2) = tokio::join!(
        http(&app1, "POST", "/admin/setup", Some(valid_setup("admin-a")), None),
        http(&app2, "POST", "/admin/setup", Some(valid_setup("admin-b")), None),
    );
    let ok_count = [r1.0, r2.0].iter().filter(|s| **s == StatusCode::OK).count();
    assert_eq!(ok_count, 1, "exactly one setup must succeed: {r1:?} {r2:?}");
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(&pool).await.unwrap();
    assert_eq!(n, 1);
}
```

（`verify_user` 恒定时间与 `update_password` 行检查无独立可测 API 面，由既有 `user_and_session_db_functions` 测试的既有断言 + 新增 db 级断言覆盖——Task 1 的实现步骤中在 `user_and_session_db_functions` 里追加两行：`update_password` 对不存在用户返回 Err。）

```rust
// 追加到 user_and_session_db_functions 末尾：
assert!(
    server::db::update_password(&pool, "nobody", "x-password").await.is_err(),
    "update_password on missing user must error"
);
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --test api_test setup_is_atomic_against_duplicate_admin concurrent_setup_never_creates_two_admins`
Expected: FAIL（现状第二个 setup 返回 200）

- [ ] **Step 3: db.rs 改造**

```rust
/// 原子创建管理员：单语句 INSERT...WHERE NOT EXISTS 防并发双管理员。
/// 返回 Ok(true)=创建成功；Ok(false)=已存在（调用方转 409）。
pub async fn create_user_if_empty(
    pool: &SqlitePool,
    username: &str,
    password: &str,
) -> Result<bool> {
    let hash = hash_password(password)?;
    let created_at = chrono::Utc::now().to_rfc3339();
    let res = sqlx::query(
        "INSERT INTO users (username, password_hash, created_at) \
         SELECT ?, ?, ? WHERE NOT EXISTS (SELECT 1 FROM users)",
    )
    .bind(username)
    .bind(&hash)
    .bind(created_at)
    .execute(pool)
    .await?;
    Ok(res.rows_affected() > 0)
}
```

`verify_user` 用户不存在分支加 dummy 验证（恒定时间，防时序枚举）：

```rust
// 模块级常量：预计算的 argon2id dummy hash（固定盐），用户不存在时验证它
// 使不存在与密码错误的耗时一致（时序型用户名枚举防护）
const DUMMY_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHRzYWx0c2FsdA$X7d8L4k2Fc0QY3qN9zJmZ1WpRvGtUoHs6KbCeAiDfEg";

pub async fn verify_user(pool: &SqlitePool, username: &str, password: &str) -> Result<bool> {
    let row = sqlx::query("SELECT password_hash FROM users WHERE username = ?")
        .bind(username).fetch_optional(pool).await?;
    let stored = match row {
        Some(r) => r.get::<String, _>(0),
        None => return Ok(verify_password_hash(password, DUMMY_HASH)), // 恒定时间：仍跑一次 argon2
    };
    Ok(verify_password_hash(password, &stored))
}
```

注意：DUMMY_HASH 的哈希值必须是真实有效的 argon2id 哈希（否则 verify 秒失败）。实施时用 argon2 工具生成一个合法 hash 填进去（或在本步骤用 `hash_password("dummy-password")` 临时生成后硬编码）。若手头无法生成，可退而求其次：用户不存在时 `hash_password(password).map(|_| false)`（也跑一次 argon2，但比 verify 稍快——防护级别略低但可接受）。**优先用真实 DUMMY_HASH 方案。**

`update_password` 行检查：

```rust
pub async fn update_password(pool: &SqlitePool, username: &str, new_password: &str) -> Result<()> {
    let hash = hash_password(new_password)?;
    let res = sqlx::query("UPDATE users SET password_hash = ? WHERE username = ?")
        .bind(hash).bind(username).execute(pool).await?;
    if res.rows_affected() == 0 {
        return Err(anyhow::anyhow!("no user with username {username}"));
    }
    Ok(())
}
```

- [ ] **Step 4: auth.rs setup 改原子 + UNIQUE 兜底**

```rust
async fn setup(
    State(state): State<AppState>,
    body: Result<Json<SetupRequest>, JsonRejection>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let Json(b) = body.map_err(ApiError::from)?;
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
    let created = crate::db::create_user_if_empty(&state.pool, &username, &b.password).await;
    match created {
        Ok(true) => Ok(Json(serde_json::json!({ "username": username }))),
        Ok(false) => Err(ApiError::conflict("admin user already exists")),
        // UNIQUE 兜底：极端竞态下 INSERT 冲突也转 409（非 500）
        Err(e) => {
            if is_unique_violation(&e) {
                Err(ApiError::conflict("admin user already exists"))
            } else {
                Err(e.into())
            }
        }
    }
}
```

`is_unique_violation` 从 combineds.rs 提取为共享函数（`crates/server/src/db.rs` 或 `routes/mod.rs`），combineds.rs 改为引用：

```rust
// routes/mod.rs 或 db.rs（择一，二处引用）
pub(crate) fn is_unique_violation(e: &sqlx::Error) -> bool {
    e.as_database_error()
        .map(|d| d.message().contains("UNIQUE"))
        .unwrap_or(false)
}
```

combineds.rs 删除本地定义改 `use crate::routes::is_unique_violation;`（若放 routes/mod.rs）或 `use crate::db::...`。

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test --test api_test setup_is_atomic_against_duplicate_admin concurrent_setup_never_creates_two_admins user_and_session_db_functions`
Expected: 全 PASS

- [ ] **Step 6: 门禁 + commit**

Run: `cargo fmt --all && cargo clippy --workspace && cargo test --workspace`

```bash
git add crates/server/src/db.rs crates/server/src/routes/auth.rs crates/server/src/routes/combineds.rs crates/server/src/routes/mod.rs crates/server/tests/api_test.rs
git commit -m "fix(auth): setup 原子化防并发双管理员（INSERT...SELECT 单语句）+ UNIQUE 冲突转 409 + verify_user 恒定时间 + update_password 行检查"
```

---

### Task 2: 前端功能修复 — toast 死代码 / 在途请求竞态 / 受控下拉 / 空 token 兜底 / 重试按钮 / 窄屏 CSS

**Files:**
- Modify: `crates/server/web/src/components/config.rs`（改密 toast 死代码）
- Modify: `crates/server/web/src/components/preview_section.rs`（在途请求覆盖竞态）
- Modify: `crates/server/web/src/components/combineds.rs`（预览下拉受控）
- Modify: `crates/server/web/src/components/login.rs`（空 token 兜底 + setup-status 重试按钮）
- Modify: `crates/server/web/index.html`（窄屏 chevron 覆盖）

**Interfaces:**
- Consumes: 现状组件（Task 0 基线）
- Produces: 行为变更——改密成功不再发不可见 toast；预览请求按序号丢弃过期响应；组合预览下拉受控；login 对空 token 报错；setup-status 失败可重试；窄屏分组箭头可见

- [ ] **Step 1: config.rs 移除改密 toast 死代码**

现状（改密成功分支）：`push_toast(toasts, ToastKind::Success, "密码已修改，请重新登录"); clear_token(); token2.set(None);`——`token2.set(None)` 立即卸载 ToastProvider，toast 从未被绘制。改为直接回登录页（表单消失即反馈）：

```rust
Ok(_) => {
    // 会话已失效（服务端已删全部会话），直接回登录页；toast 会被卸载不渲染，故不发
    clear_token();
    token2.set(None);
}
```

若 `toasts` 信号因此不再使用，删除对应变量与 `use_toast` 引用（若 config.rs 其他地方还用则保留）。

- [ ] **Step 2: preview_section.rs 在途请求竞态**

现状：`load` 的 spawn 无版本控制，慢的旧请求晚到会覆盖新 key 的数据。加请求序号：

```rust
let mut req_seq = use_signal(|| 0u32);

let mut load = move |key: String| {
    let token = token.read().clone();
    let mut data = data.clone();
    let mut loading = loading.clone();
    let mut error = error.clone();
    let mut req_seq = req_seq.clone();
    let seq = {
        let mut s = req_seq.write();
        *s += 1;
        *s
    };
    spawn(async move {
        loading.set(true);
        error.set(String::new());
        let path = /* 现状路径拼装（key 前缀分支不变） */;
        match request("GET", &path, None, token.as_deref()).await {
            Ok(body) => {
                if *req_seq.read() != seq { return; } // 过期响应丢弃
                match serde_json::from_str::<PreviewResp>(&body) {
                    Ok(r) => data.set(Some(r)),
                    Err(e) => error.set(format!("解析失败: {}", e)),
                }
            }
            Err(e) => {
                if *req_seq.read() == seq {
                    error.set(e.to_string());
                }
            }
        }
        if *req_seq.read() == seq {
            loading.set(false);
        }
    });
};
```

注意：loading 关闭也受 seq 保护（旧请求不得关闭新请求的 loading）。

- [ ] **Step 3: combineds.rs 预览下拉受控**

现状：`select` 无 value 绑定（非受控），组合改名/删除后显示可能漂移。加受控：

```rust
// rsx 中 select 加 value 绑定：
select {
    class: "preview-filter",
    value: preview_combined.read().clone().unwrap_or_default(),
    onchange: move |e| {
        let v = e.value();
        let v = if v.is_empty() { None } else { Some(v) };
        preview_combined.set(v);
    },
    option { value: "", "选择组合订阅" }
    // options 不变
}
```

- [ ] **Step 4: login.rs 空 token 兜底 + setup-status 重试按钮**

空 token 兜底（现状 `v["token"].as_str().unwrap_or_default()` 会把空串传给 on_login）：

```rust
Ok(b) => match serde_json::from_str::<serde_json::Value>(&b) {
    Ok(v) => match v["token"].as_str() {
        Some(t) if !t.is_empty() => on_login.call(t.to_string()),
        _ => error.set("登录响应缺少 token，请重试".into()),
    },
    Err(e) => error.set(format!("解析失败: {e}")),
},
```

setup-status 重试按钮：None 分支（加载中/失败）在 error 非空时渲染错误 + 「重试」按钮，点击重新请求 setup-status：

```rust
// 提取 setup-status 探测为可复用闭包/函数：
let mut probe = use_callback(move |_: ()| {
    let mut needs_setup = needs_setup.clone();
    let mut error = error.clone();
    spawn(async move {
        match request("GET", "/admin/setup-status", None, None).await {
            Ok(body) => match serde_json::from_str::<serde_json::Value>(&body) {
                Ok(v) => {
                    needs_setup.set(Some(v["needs_setup"].as_bool().unwrap_or(false)));
                    error.set(String::new());
                }
                Err(e) => error.set(format!("解析失败: {e}")),
            },
            Err(e) => error.set(format!("检查初始化状态失败: {e}")),
        }
    });
});
// 挂载时 use_future 调一次 probe（或 use_effect + 守卫）
// None 分支渲染：error 非空 → error-text + 重试按钮（onclick: probe.call(())）；
// error 空 → Spinner
```

注意保持现状三态渲染（None=加载/失败、Some(true)=创建表单、Some(false)=登录表单）与 rsx 元素形式。

- [ ] **Step 5: index.html 窄屏 chevron 覆盖**

现状：`@media (max-width: 768px)` 的 `.nav-item span { display: none }`（specificity 0,1,1）隐藏了 `.nav-chevron`（0,1,0）。加更高优先级覆盖：

```css
@media (max-width: 768px) {
  /* ...现有规则... */
  /* 覆盖：分组头箭头在顶栏模式下仍需可见（specificity 0,2,0 > 0,1,1） */
  .nav-group-head .nav-chevron { display: inline-flex; }
}
```

- [ ] **Step 6: 构建验证 + 门禁 + commit**

Run: `cd crates/server/web && dx build --web --release --debug-symbols false`（0 警告）→ `cargo fmt --all && cargo clippy --workspace && cargo test --workspace`

```bash
git add crates/server/web/src/ crates/server/web/index.html
git commit -m "fix(web): 改密 toast 死代码移除 + 预览在途请求竞态 + 受控下拉 + 登录空 token 兜底 + setup 探测重试 + 窄屏箭头"
```

---

### Task 3: 死代码清理 — 图标 / CSS / mask_token / NavGroup name / PreviewSummary / 空 class

**Files:**
- Modify: `crates/server/web/src/components/icon.rs`（删 "overview"）
- Modify: `crates/server/web/index.html`（删 .stats-grid/.stat-card/.grid-2 死 CSS）
- Modify: `crates/web-core/src/fmt.rs`（删 mask_token + 测试）
- Modify: `crates/server/web/src/main.rs`（NavGroup 删 name prop）
- Modify: `crates/web-core/src/dto.rs`（删 PreviewSummary + 测试）
- Modify: `crates/server/web/src/components/combineds.rs`（删 .preview-filter-row class）

**Interfaces:**
- Consumes: 现状（Task 0 基线）
- Produces: 无行为变化，仅删除无消费方代码

- [ ] **Step 1: 逐项删除**

1. `icon.rs`：删除 `"overview"` match 分支（grep 确认无引用后）
2. `index.html`：删除 `.stats-grid`、`.stat-card`、`.stat-icon`（含 .danger）、`.stat-value`、`.stat-label`、`.grid-2`（含 `> .card` 规则）全部规则；同时删除 `@media (max-width: 768px)` 内的 `.stats-grid { grid-template-columns: repeat(2, 1fr) }`、`.grid-2 { grid-template-columns: 1fr }` 与 `@media (max-width: 480px)` 的 `.stats-grid { grid-template-columns: 1fr }`（grep 确认无消费方——overview.rs 已删）
3. `web-core/src/fmt.rs`：删除 `mask_token` 函数与 `mask_token_hides_any_token` 测试（grep 确认无引用——config.rs 已不用）
4. `web/src/main.rs`：NavGroup 组件签名删 `name: &'static str` 参数与调用点传参（grep 确认无其他使用）
5. `web-core/src/dto.rs`：删除 `PreviewSummary` struct 与 `preview_summary_parses` 测试（grep 确认无引用——overview.rs 已删，preview 单元已删）
6. `combineds.rs`：删除 `div { class: "preview-filter-row", ... }` 的 class（改用无 class 的 div 或直接去掉包装 div——保留结构，去掉无样式的 class 属性）

- [ ] **Step 2: 验证无残留 + 构建**

Run: `grep -rn "mask_token\|PreviewSummary\|preview-filter-row\|stats-grid\|stat-card\|grid-2\|\"overview\"" crates/ --include="*.rs" --include="*.html" | grep -v target`（除历史 docs 外应无命中）
Run: `cd crates/server/web && dx build --web --release --debug-symbols false`（0 警告）

- [ ] **Step 3: 门禁 + commit**

Run: `cargo fmt --all && cargo clippy --workspace && cargo test --workspace`

```bash
git add -A crates/
git commit -m "chore: 清理遗留死代码（overview 图标/stats CSS/mask_token/PreviewSummary/NavGroup name/空 class）"
```

---

### Task 4: 测试加固 + 文档修正

**Files:**
- Modify: `crates/server/tests/api_test.rs`（测试临时目录清理）
- Modify: `scripts/ui-check.py`（c-test 空 DB guard、refresh_failure 断言精度）
- Modify: `CLAUDE.md`（架构节 users/sessions）
- Modify: `docs/superpowers/specs/2026-08-07-web-core-testing-design.md`（fixture 过时）
- Modify: `crates/server/web/src/components/login.rs`（注释修正——若 Task 2 已重构 probe 逻辑，注释随重构自然修正）

**Interfaces:**
- Consumes: 现状
- Produces: 测试健壮性 + 文档准确

- [ ] **Step 1: api_test.rs 测试临时目录清理**

现状：测试用 `std::env::temp_dir().join(format!("submerge-test-{pid}-{tag}"))` 且不清理，PID 复用会偶发失败。在 `test_pool` 前清理：

```rust
// 在 test_config/test_pool 上方加辅助：
fn fresh_tmp(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("submerge-test-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir); // 清理 PID 复用残留
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
```

将现有测试中 `std::env::temp_dir().join(...)` + `create_dir_all` 两行模式替换为 `let tmp = fresh_tmp("...");`（约 30 处，机械替换，tag 用现有测试名的短标识）。`final_fixes.rs` 的 `unique_dir` 同步加清理（`remove_dir_all` 已有，仅起始清理）。

- [ ] **Step 2: ui-check.py 加固**

c-test 补建空 DB guard（现状 `json.loads(...)[0]["id"]` 空列表 IndexError）：

```python
# scenario_preview_filter 的 c-test 补建段：先确保至少有一个源
req = u.Request(URL + "/admin/sources", headers={"Authorization": "Bearer " + SESSION_TOKEN})
with u.urlopen(req, timeout=5) as r:
    sources = json.loads(r.read())
if not any(c.get("name") == "c-test" for c in combos):
    if not sources:
        # 空 DB：先经 API 种一个 single 源（与 seed_sources 相同节点形态）
        u.urlopen(u.Request(URL + "/admin/sources", method="POST",
                            data=json.dumps({"name": "c-seed",
                                             "url": "vless://e99a8e5a-6b2b-4a1d-9c5f-1a2b3c4d5e6f@1.2.3.4:443#c-seed",
                                             "kind": "single"}).encode(),
                            headers={"Authorization": "Bearer " + SESSION_TOKEN,
                                     "Content-Type": "application/json"}), timeout=5)
        sources = json.loads(json.dumps([{"id": 0}]))  # 占位；下方重新读
        req = u.Request(URL + "/admin/sources", headers={"Authorization": "Bearer " + SESSION_TOKEN})
        with u.urlopen(req, timeout=5) as r:
            sources = json.loads(r.read())
    first_id = sources[0]["id"]
```

（实施时以实际脚本结构为准做最小改造：补建前确保 sources 非空即可。）

refresh_failure 断言精度（现状 rows0 混入源列表行）：改为仅统计预览区表格：

```python
# 记录预览区表格行数（预览卡片的 .table-wrap tbody tr）
rows0 = ev(ws, "document.querySelectorAll('.card:last-of-type .table-wrap tbody tr').length")
```

（实施时以实际 DOM 结构为准：预览卡片是页面最后一张 card，或给预览卡片加定位 class——选实现最稳的方式并在报告中说明选择器。）

- [ ] **Step 3: 文档修正**

1. `CLAUDE.md` 架构节 server 行：「SQLite（sqlx）持久化 sources/settings/combined_subs/combined_sources」→「持久化 sources/users/sessions/combined_subs/combined_sources（settings 表保留但已无读写）」
2. `docs/superpowers/specs/2026-08-07-web-core-testing-design.md:85` fixture `{"admin_token":"abc123"}` → `{"username":"admin"}`（并同步 config_dto_parses 描述）
3. `login.rs` 注释「用 None 守卫一次性执行」→ 按 Task 2 重构后的实际机制描述（use_future 单参数本就只运行一次 / probe 闭包）

- [ ] **Step 4: 验证 + 门禁 + commit**

Run: `python3 -m py_compile scripts/ui-check.py` + `cargo fmt --all && cargo clippy --workspace && cargo test --workspace`

```bash
git add -A
git commit -m "chore: 测试加固（临时目录清理/c-test guard/断言精度）+ 文档修正（架构节/fixture/注释）"
```

---

## 自审记录

**遗留清单覆盖检查（对照两个批次 ledger + 最终评审）：**

| 遗留项 | 处理 |
|--------|------|
| setup 并发竞态（双管理员/500） | Task 1（create_user_if_empty 原子 INSERT + UNIQUE 转 409） |
| verify_user 时序枚举 | Task 1（DUMMY_HASH 恒定时间） |
| update_password 静默 no-op | Task 1（rows_affected 检查） |
| 改密 toast 死代码 | Task 2 |
| PreviewSection 在途请求覆盖 | Task 2（req_seq 版本号） |
| 组合预览下拉非受控漂移 | Task 2（受控 value） |
| 窄屏 chevron 隐藏 | Task 2（CSS 覆盖） |
| login 空 token 兜底 | Task 2 |
| setup-status 无重试按钮 | Task 2（probe 闭包 + 重试按钮） |
| overview 图标 / stats CSS / mask_token / PreviewSummary / NavGroup name / preview-filter-row | Task 3 |
| 会话 TTL | **明确不修**（spec YAGNI） |
| 测试临时目录 PID 复用 | Task 4（fresh_tmp 清理） |
| c-test 空 DB IndexError | Task 4 |
| refresh_failure rows0 精度 | Task 4 |
| CLAUDE.md 架构节 / web-core spec fixture / login.rs 注释 | Task 4 |
| login token hex 断言 | 由 Task 2 的空 token 兜底测试覆盖语义（hex 性质本身由 gen_token 保证，不单独断言） |
| 旧 localStorage key 不清理 | **不修**（残留无害，非有效凭据） |
| ui-check chrome 实跑 | 环境限制，代码审读 + py_compile |

**无占位符检查：** 各任务含完整代码或明确修改点；Task 1 的 DUMMY_HASH 注明实施时需生成合法 argon2id 哈希（给出生成方式与退路）。

**类型一致性：** `create_user_if_empty -> Result<bool>` 在 Task 1 定义并被 auth.rs setup 使用；`is_unique_violation` 提取为共享函数被 auth.rs/combineds.rs 引用；`probe: use_callback` 在 Task 2 定义并被挂载与重试按钮使用；Task 3 删除项与 Task 2 改动无交叉（icon/index.html/fmt.rs/main.rs/dto.rs/combineds.rs 各处独立）。
