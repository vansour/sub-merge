# C 组遗留修复 + A 组 ui-check 实跑计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复遗留清单 C 组全部 8 项（代码小修）+ A 组（自动安装 chrome-headless-shell 环境并实跑 ui-check.py 全套场景，修复暴露的问题）。

**Architecture:** 三个任务：后端 5 项（401 文案区分、迁移回填分支、秒精度 cutoff、i64 回绕、token hex 断言）→ 前端 3 项（probe 在途去重、组合删除后预览选择重置、窄屏分组默认收起）→ A 组环境（apt 装 chromium/chrome-headless-shell + pip websocket-client → 构建前端 → 起 server + chrome → 跑 7 个场景 → 修暴露问题 → 复跑全绿）。

**Tech Stack:** axum/sqlx（后端）；dioxus 0.8.0-alpha.1（前端）；ui-check.py（CDP）；chrome-headless-shell；python websocket-client。

## Global Constraints

- 每次代码修改后必须按序通过：`cargo upgrade -i` → `cargo fmt --all` → `cargo clippy --workspace` → `cargo test --workspace`（CLAUDE.md 强制）
- Rust edition 2024，rust-version 1.97
- web crate 独立于 workspace（不进 members），仅由 `dx build --web --release --debug-symbols false` 构建，必须 0 警告
- dioxus 0.8 坑清单：rsx if/else 分支内不嵌套 `rsx!` 宏；use_effect 只在挂载 + 被读信号变化时重跑；Signal::set 需 let mut
- ui-check.py 前置（脚本头部注释）：server 运行在 :18080（首次运行引导创建管理员，脚本 login() 自动完成 setup+login）；`chrome-headless-shell --headless --no-sandbox --remote-debugging-port=9222 --remote-allow-origins=* about:blank`
- C 组修复清单（本计划依据）：
  1. auth.rs 401 文案坍缩（区分 missing/invalid/expected Bearer 三种）
  2. 窄屏顶栏分组默认收起
  3. DUMMY_HASH 密码公开（注释已警示——保持现状，不动）
  4. login probe 在途去重
  5. 组合删除后预览选择重置
  6. 迁移回填 UPDATE 移入 ALTER 成功分支
  7. delete_expired_sessions 秒精度 cutoff
  8. ttl_days as i64 回绕加固
  9. c-test guard 理论盲区（防御性注释，不动）
  10. login token hex 断言
  （第 3、9 项为「不修仅记录」——执行时验证现状即可，不产生代码改动）

---

### Task 1: C 组后端 5 项

**Files:**
- Modify: `crates/server/src/auth.rs`（401 文案区分）
- Modify: `crates/server/src/db.rs`（回填分支、秒精度、i64 加固）
- Test: `crates/server/tests/api_test.rs`（token hex 断言）

**Interfaces:**
- Consumes: 现状（Task 0 基线）
- Produces: `extract_bearer` 语义不变（返回值仍 Option<&str>），401 文案区分在 require_admin 内实现；`delete_expired_sessions`/`validate_session` 行为不变（仅内部加固）

- [ ] **Step 1: auth.rs 401 文案区分**

现状：`extract_bearer` 对「无 header / 非 UTF-8 / 非 Bearer 前缀」统一返回 None → require_admin 一律「missing authorization header」。改为区分三种：

```rust
pub fn extract_bearer(headers: &HeaderMap) -> Option<&str> {
    let auth = headers.get(AUTHORIZATION)?.to_str().ok()?;
    auth.strip_prefix("Bearer ")
}
```

改造为返回可区分结果（保留 extract_bearer 供 logout 复用，新增独立解析在 require_admin）：

```rust
/// 解析 Bearer 头。返回三态：Ok(token) / Err(原因) / 无 header 时 Err("missing")
pub fn parse_bearer(headers: &HeaderMap) -> Result<&str, &'static str> {
    let auth = headers
        .get(AUTHORIZATION)
        .ok_or("missing authorization header")?;
    let auth_str = auth.to_str().map_err(|_| "invalid authorization header")?;
    auth_str
        .strip_prefix("Bearer ")
        .ok_or("expected Bearer token")
}

// extract_bearer 改为 parse_bearer 的 Option 包装（logout 复用）：
pub fn extract_bearer(headers: &HeaderMap) -> Option<&str> {
    parse_bearer(headers).ok()
}

pub async fn require_admin(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(), ApiError> {
    let token = match parse_bearer(&headers) {
        Ok(t) => t,
        Err(msg) => return Err(ApiError::unauthorized(msg)),
    };
    if crate::db::validate_session(&state.pool, token, state.cfg.session_ttl_days).await? {
        Ok(())
    } else {
        Err(ApiError::unauthorized("invalid session"))
    }
}
```

- [ ] **Step 2: db.rs 三项加固**

回填 UPDATE 移入 ALTER 成功分支（现状无条件执行）：

```rust
// init_db 内：
let mut migrated = false;
if let Err(e) = sqlx::query(
    "ALTER TABLE sessions ADD COLUMN last_used_at TEXT NOT NULL DEFAULT ''",
)
.execute(&pool)
.await
{
    if !e.to_string().contains("duplicate column name") {
        return Err(e.into());
    }
    // 列已存在：不执行回填（旧库迁移只发生一次）
} else {
    migrated = true;
}
if migrated {
    // 仅在本次 ALTER 实际成功（旧库首次迁移）时回填
    sqlx::query(
        "UPDATE sessions SET last_used_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE last_used_at = ''",
    )
    .execute(&pool)
    .await?;
}
```

注意：新库（CREATE TABLE 自带 last_used_at）时 ALTER 会报 duplicate column → migrated=false，回填跳过（新库无空值，正确）。旧库首次启动 → ALTER 成功 → 回填。实现时核对现有 init_db 的 ALTER 代码结构（sources.kind 迁移是 `if let Err(e) = ... && !contains` 模式，按实际文件结构适配）。

秒精度 cutoff（`delete_expired_sessions`）：

```rust
let cutoff = (chrono::Utc::now() - chrono::Duration::days(ttl_days as i64))
    .with_nanosecond(0) // 截断到秒精度，与迁移回填/存储格式对齐
    .unwrap_or_else(chrono::Utc::now)
    .to_rfc3339();
```

i64 回绕加固（`validate_session` 与 `delete_expired_sessions` 的 `ttl_days as i64`）：

```rust
let ttl = i64::try_from(ttl_days).unwrap_or(i64::MAX);
// validate_session: chrono::Duration::days(ttl)
// delete_expired_sessions: chrono::Duration::days(ttl)
```

- [ ] **Step 3: token hex 断言（api_test.rs）**

`login_and_logout_flow` 的 token 断言处追加（或新建断言）：

```rust
// 现有: assert_eq!(token.len(), 64);
// 追加 hex 字符集断言:
assert!(
    token.chars().all(|c| c.is_ascii_hexdigit()),
    "token must be hex: {token}"
);
```

- [ ] **Step 4: 验证 + 门禁 + commit**

Run: `cargo fmt --all && cargo clippy --workspace && cargo test --workspace`（基线 140 + 无新增测试数变化——断言增强在既有测试内）

```bash
git add crates/server/src/auth.rs crates/server/src/db.rs crates/server/tests/api_test.rs
git commit -m "fix(auth): 401 文案区分三态 + TTL 边界加固（秒精度/i64 回绕/回填分支）+ token hex 断言"
```

---

### Task 2: C 组前端 3 项

**Files:**
- Modify: `crates/server/web/src/components/login.rs`（probe 在途去重）
- Modify: `crates/server/web/src/components/combineds.rs`（组合删除后预览选择重置）
- Modify: `crates/server/web/src/main.rs`（窄屏分组默认收起——初始展开状态按宽度）
- Modify: `crates/server/web/index.html`（如需配合 CSS）

**Interfaces:**
- Consumes: 现状
- Produces: probe 单飞（重试连点不并发）；preview_combined 失效自动清空；窄屏（≤768px）初始收起两分组

- [ ] **Step 1: login.rs probe 在途去重**

现状 probe 是 use_callback，无 in-flight 守卫。加探针中信号（参照 DataStore 单飞模式）：

```rust
// 状态区追加：
let mut probing = use_signal(|| false);

let mut probe = use_callback(move |_: ()| {
    if probing() { return; } // 单飞：在途探测不重复发起
    probing.set(true);
    let mut needs_setup = needs_setup.clone();
    let mut error = error.clone();
    let mut probing = probing.clone();
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
        probing.set(false);
    });
});
// 重试按钮可加 disabled: *probing.read()（可选，与 loading 模式一致）
```

- [ ] **Step 2: combineds.rs 预览选择重置**

组合列表变化（删除/改名）后，`preview_combined` 若指向不存在的组合名 → 清空。用 use_effect 监听 combineds 单元数据（use_effect 在读信号变化时重跑——读 `combineds_state.data` 即可触发）：

```rust
// 组合列表变化时，选中值不在列表则重置
let combined_names: Vec<String> = combined_list.iter().map(|c| c.name.clone()).collect();
use_effect(move || {
    if let Some(sel) = preview_combined.read().clone() {
        if !combined_names.contains(&sel) {
            preview_combined.set(None);
        }
    }
});
```

注意：`combined_names` 是普通 Vec（非响应式），use_effect 重跑需要读响应式信号——`combined_list` 来自 `combineds_state.data`（Signal read），effect 内读 `combineds_state` 的派生值即可触发。实施时确认 effect 内读到了信号（如直接读 `data.combineds.read()`），避免守卫不触发。若 use_effect 触发语义不符，改用 use_reactive 桥接 `combined_names`（参照 preview_section.rs 模式）。

- [ ] **Step 3: main.rs 窄屏默认收起**

初始展开状态按窗口宽度决定：

```rust
// MainShell 内，open_groups 初始化处：
let is_narrow = web_sys::window()
    .and_then(|w| w.match_media("(max-width: 768px)").ok().flatten())
    .map(|m| m.matches())
    .unwrap_or(false);
let default_open: std::collections::HashSet<&'static str> =
    if is_narrow { std::collections::HashSet::new() } else { std::collections::HashSet::from(["subs", "single"]) };
let mut open_groups = use_signal(|| default_open);
```

（web-sys 的 Window::match_media 返回 Result<Option<MediaQueryList>, JsValue>——按实际签名适配；`use_signal(|| default_open)` 的初始化闭包捕获 is_narrow 即可。）

- [ ] **Step 4: 验证 + 门禁 + commit**

Run: `cd crates/server/web && dx build --web --release --debug-symbols false`（0 警告）→ `cargo fmt --all && cargo clippy --workspace && cargo test --workspace`

```bash
git add crates/server/web/src/
git commit -m "fix(web): probe 在途去重 + 组合删除后预览选择重置 + 窄屏分组默认收起"
```

---

### Task 3: A 组 — 安装环境并实跑 ui-check.py 全套

**Files:**
- Modify: `scripts/ui-check.py`（实跑暴露的问题修复——预期 1-3 处断言微调）
- 环境：安装 chrome-headless-shell（或 chromium）+ python websocket-client

**Interfaces:**
- Consumes: Task 1/2 与既有全部功能
- Produces: ui-check.py 7 个场景在真实 chrome 下全部 PASS（修复暴露的断言/时序问题）

- [ ] **Step 1: 安装环境**

```bash
# chrome-headless-shell（Google 官方 standalone 二进制，免 apt chromium 的依赖泥潭）
mkdir -p /opt/chrome-headless-shell && cd /opt/chrome-headless-shell
# 下载 chrome-headless-shell（linux64）——用最新 stable 版本 URL
curl -sSLo chrome-headless-shell.tar.gz <chrome-headless-shell linux64 下载地址>
tar -xzf chrome-headless-shell.tar.gz
# 依赖：debian 容器可能需要
apt-get update && apt-get install -y --no-install-recommends ca-certificates fonts-liberation libnss3 libnspr4 libatk1.0-0 libatk-bridge2.0-0 libcups2 libdrm2 libxkbcommon0 libxcomposite1 libxdamage1 libxfixes3 libxrandr2 libgbm1 libasound2 libpango-1.0-0 libcairo2

# python 依赖
pip install websocket-client
```

（若 chrome-headless-shell 下载不便，退路：`apt-get install -y chromium` + `--no-sandbox`。安装后验证 `chrome-headless-shell --version`。）

- [ ] **Step 2: 构建前端 + 起 server**

```bash
cd crates/server/web && dx build --web --release --debug-symbols false
cd /root/github/sub-merge/.claude/worktrees/<本批次 worktree>
WEB_DIST=./crates/server/web/dist DATABASE_PATH=/tmp/ui-check.db PORT=18080 cargo run -p server &
# 等 healthz 200
curl -sf http://127.0.0.1:18080/healthz
```

- [ ] **Step 3: 起 chrome + 按序跑 7 场景**

```bash
chrome-headless-shell --headless --no-sandbox --remote-debugging-port=9222 --remote-allow-origins=* about:blank &
# 场景顺序（依赖关系：combineds 建 c-test → preview_filter 用；refresh_failure 会停/重启 server）
python3 scripts/ui-check.py nav_preload
python3 scripts/ui-check.py sources_crud
python3 scripts/ui-check.py combineds
python3 scripts/ui-check.py preview_filter
python3 scripts/ui-check.py refresh_failure
python3 scripts/ui-check.py config_password
python3 scripts/ui-check.py first_load_failure
```

每个场景期望 `== <场景名>: ALL PASS ==`。

- [ ] **Step 4: 修复暴露的问题**

实跑失败的场景按序诊断：
- **时序类**（wait_until 超时、转圈断言）：调 timeout/interval 或调整断言表达式
- **选择器类**（element 未找到）：对照实际 DOM 修正选择器
- **行为类**（真 bug）：修复代码（前端组件/后端逻辑），跑对应门禁
每处修复记录在报告（问题现象 → 根因 → 修复 → 复跑结果）。修复属于 ui-check.py 的改 `scripts/ui-check.py`；属于产品代码的改对应组件并跑门禁。

- [ ] **Step 5: 全套复跑确认 + 门禁 + commit**

7 场景全 PASS 后：
Run: `cargo fmt --all && cargo clippy --workspace && cargo test --workspace`

```bash
git add -A
git commit -m "chore(ui-check): 实跑 7 场景修复暴露问题（<摘要>）"
```

若实跑零暴露（全过），本任务 commit 可省略或仅记录环境准备说明。

---

## 自审记录

**C 组覆盖检查（10 项清单）：**
| 项 | 处理 |
|----|------|
| 401 文案坍缩 | Task 1（parse_bearer 三态） |
| 窄屏分组默认收起 | Task 2（matchMedia 初始化） |
| DUMMY_HASH 密码公开 | 不修（注释已警示，当前实现安全）——Task 1 验证现状 |
| login probe 在途去重 | Task 2 |
| 组合删除后预览选择重置 | Task 2（use_effect 监听列表） |
| 回填 UPDATE 分支 | Task 1（migrated 标志） |
| 秒精度 cutoff | Task 1（with_nanosecond(0)） |
| i64 回绕 | Task 1（try_from + i64::MAX） |
| c-test guard 理论盲区 | 不修（防御性，实际不可能）——Task 3 实跑时自然验证 |
| token hex 断言 | Task 1（is_ascii_hexdigit） |

**A 组：** Task 3 覆盖（环境安装 → 实跑 → 修复 → 复跑）；失败场景的修复路径按「时序/选择器/行为」三类给出诊断指引，无占位符。

**类型一致性：** `parse_bearer -> Result<&str, &'static str>` 在 Task 1 定义，require_admin/logout 使用一致；`probing: Signal<bool>` 在 Task 2 login.rs 定义并被 probe/重试按钮引用；`is_narrow` 在 Task 2 main.rs 初始化 open_groups。
