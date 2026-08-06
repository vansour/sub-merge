# 依赖健康与复杂度硬化实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 按 2026-08-07 三视角审查（依赖健康/deprecated API/时间复杂度）结论，修复全部 10 项：5 项依赖迁移（serde_yaml、mime_guess、hex、urlencoding、gloo-net）+ 5 项复杂度/正确性优化（N+1、批量插入、create 事务、IN 超限吞错、base64 单遍、前端 HashSet）。

**Architecture:** proxy-core 依赖替换保持 API 同形（serde_yaml_ng 为 serde_yaml 0.9 的维护 fork；urlencoding 用 percent-encoding 自定义 AsciiSet 保持等价语义）；server 依赖用 package 重命名（mime_guess2）与 const-hex 替换；后端 SQL 批量/聚合消除 N+1 与逐成员查询；前端 checked 换 HashSet。

**Tech Stack:** Rust 2024 / axum 0.8 / sqlx 0.9 / dioxus 0.8.0-alpha.1（web）

## Global Constraints

- 每次代码修改后必须按顺序全部通过：`cargo upgrade -i`、`cargo fmt --all`、`cargo clippy --workspace`、`cargo test --workspace`（web crate 单独 `dx build --web --release`）
- Rust edition 2024；不得引入旧 edition
- web crate 独立于 workspace，唯一构建方式 `cd crates/server/web && dx build --web --release`
- dioxus 0.8 alpha 坑约束（rsx 分支不嵌套宏等）
- 前端无测试 harness，验证 = dx build + make smoke + 浏览器人工核对
- serde_yaml 替代品已决策：**serde_yaml_ng 0.10**（唯一无 deprecated 标记、API 完全 drop-in、维护者活跃的选项；noyalib 0.0.x 太新、serde-saphyr 需重写 Value DOM 排除）
- 所有迁移必须保持行为等价：urlencoding→percent-encoding 用自定义 AsciiSet（保留 -_.~）；既有 roundtrip/解析测试必须全绿

---

### Task 1: proxy-core 依赖迁移（serde_yaml_ng + urlencoding→percent-encoding）

**Files:**
- Modify: `crates/proxy-core/Cargo.toml`（serde_yaml → serde_yaml_ng = "0.10"；删除 urlencoding，保留 percent-encoding）
- Modify: `crates/proxy-core/src/uri.rs`（新增 urlencode helper）
- Modify: `crates/proxy-core/src/parser.rs`（serde_yaml:: → serde_yaml_ng::）
- Modify: `crates/proxy-core/src/formats/clash.rs`（同上 + urlencoding 替换）
- Modify: `crates/proxy-core/src/protocols/{hysteria,hysteria2,tuic,vless,trojan,wireguard}.rs`（urlencoding → crate::uri::urlencode）
- Modify: `crates/proxy-core/tests/formats.rs`（serde_yaml:: → serde_yaml_ng::）

- [ ] **Step 1: Cargo.toml 依赖调整**

```toml
serde_yaml_ng = "0.10"
```
删除 `urlencoding = "2"` 与 `serde_yaml = "0.9"`（percent-encoding 已存在）。

- [ ] **Step 2: uri.rs 新增 urlencode（与 urlencoding::encode 语义等价）**

```rust
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

/// 与 urlencoding::encode 语义等价：保留 RFC3986 unreserved（字母数字与 -_.~），
/// 其余字符（含空格、UTF-8 多字节）逐字节 percent-encode（空格 → %20 而非 +）。
const URLENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

pub fn urlencode(s: &str) -> String {
    utf8_percent_encode(s, URLENCODE_SET).to_string()
}
```

- [ ] **Step 3: 替换 6 个协议文件的 urlencoding 使用**

每个文件：`use urlencoding::encode;` → `use crate::uri::urlencode;`（或并入现有 `use crate::uri::{...}`），调用处 `encode(x)` → `urlencode(x)`。涉及文件与调用点：hysteria.rs（2 处）、hysteria2.rs（1 处）、tuic.rs（3 处）、vless.rs（4 处）、trojan.rs（4 处）、wireguard.rs（2 处）。

- [ ] **Step 4: serde_yaml_ng 替换**

- `parser.rs`：`use serde_yaml::Value` → `serde_yaml_ng::Value`；`serde_yaml::from_str` → `serde_yaml_ng::from_str`（2 处）
- `formats/clash.rs`：`serde_yaml::to_string` → `serde_yaml_ng::to_string`（clash_yaml_str 内）
- `tests/formats.rs`：`serde_yaml::from_str` → `serde_yaml_ng::from_str`

- [ ] **Step 5: 运行验证**

Run: `cargo test --workspace`（proxy-core 全部 roundtrip/解析测试必须绿——等价性保证）+ `cargo fmt --all && cargo clippy --workspace`。
另外在 tests/formats.rs 或 protocol 测试补一条 urlencode 等价性断言（空格与保留字符集）：`assert_eq!(urlencode("a b/c~"), "a%20b%2Fc~")`。

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "chore(proxy-core): replace deprecated serde_yaml with serde_yaml_ng, urlencoding with percent-encoding
```
（提交信息结尾加 `Co-Authored-By: Claude <noreply@anthropic.com>`）

---

### Task 2: server 依赖迁移 + 后端优化

**Files:**
- Modify: `crates/server/Cargo.toml`（mime_guess package 重命名、hex → const-hex）
- Modify: `crates/server/src/db.rs`（hex::encode → const_hex::encode）
- Modify: `crates/server/src/routes/combineds.rs`（list N+1 → 单查询聚合；insert_members → 批量 INSERT...SELECT；create 事务化）
- Modify: `crates/server/src/service.rs`（IN 超限静默吞错 → SourceError）
- Modify: `crates/proxy-core/src/uri.rs`（decode_base64_url 单遍转换）
- Test: `crates/server/tests/api_test.rs`（既有组合测试保持绿；新增批量成员与 N+1 行为无变化的隐式覆盖）

- [ ] **Step 1: Cargo.toml 依赖调整**

```toml
mime_guess = { package = "mime_guess2", version = "2.3" }
const-hex = "0.15"
```
删除 `hex = "0.4"`。（package 重命名后代码 `mime_guess::from_path` 照旧，static.rs 零改动。）

- [ ] **Step 2: db.rs token 生成**

```rust
// hex::encode(buf) → const_hex::encode(buf)（API 同形，const fn）
```

- [ ] **Step 3: combineds.rs 三处优化**

1. `list_combineds`：成员查询改为单条 `SELECT combined_id, source_id FROM combined_sources ORDER BY combined_id, source_id`，内存按 combined_id 分组（`HashMap<i64, Vec<i64>>`），消除 N+1。
2. `insert_members` 改为批量单条 SQL（同时过滤不存在的源）：

```rust
/// 批量插入成员：单条 INSERT...SELECT 同时过滤不存在的源（幂等，PK 冲突由 OR IGNORE 处理）
async fn insert_members_sql(
    conn: &mut sqlx::SqliteConnection,
    combined_id: i64,
    source_ids: &[i64],
) -> Result<(), ApiError> {
    if source_ids.is_empty() {
        return Ok(());
    }
    let placeholders = std::iter::repeat_n("?", source_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let mut q = sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT OR IGNORE INTO combined_sources (combined_id, source_id) SELECT ?, id FROM sources WHERE id IN ({placeholders})"
    )));
    q = q.bind(combined_id);
    for sid in source_ids {
        q = q.bind(sid);
    }
    q.execute(conn).await?;
    Ok(())
}
```

3. `create_combined` 与 `update_combined` 成员替换统一走 `insert_members_sql`（事务内 `&mut *tx` 传入）；`create_combined` 的主表 INSERT 与成员插入包进同一事务（消除"孤立组合行"窗口）；`update_combined` 的事务块内联批量 SQL 改用同一 helper。

- [ ] **Step 4: service.rs IN 超限不吞错**

`fetch_and_merge` 的 `Some(ids) if !ids.is_empty()` 分支：`fetch_all` 改为 match——`Err(e)` 时 push `SourceError { source_name: "combined".into(), reason: format!("member query failed: {e}") }` 并返回空行，不再 `unwrap_or_default()` 静默吞掉（组合订阅不会静默变空 200）。errors 变量需在查询前初始化。

- [ ] **Step 5: uri.rs decode_base64_url 单遍**

改为单遍 `bytes().map()` 转换（URL-safe → standard 字母表）+ 尾部 padding 处理（保持 4MB 上限与长度 %4 校验），消除 3 次中间分配。行为不变（既有 base64 测试必须绿）。

- [ ] **Step 6: 运行验证 + Commit**

Run: `cargo test --workspace && cargo fmt --all && cargo clippy --workspace` 全绿。

```bash
git add -A
git commit -m "perf(server): batch member queries, single-pass base64, surface IN-limit errors; deps mime_guess2/const-hex
```
（提交信息结尾加 `Co-Authored-By: Claude <noreply@anthropic.com>`）

---

### Task 3: web 依赖升级 + 前端优化

**Files:**
- Modify: `crates/server/web/Cargo.toml`（gloo-net 0.6 → 0.7）
- Modify: `crates/server/web/src/components/combineds.rs`（checked Vec → HashSet）

- [ ] **Step 1: gloo-net 0.7**

`gloo-net = { version = "0.7", features = ["http"] }`。api.rs 的 Request/RequestBuilder/Method 用法如编译不兼容则按 0.7 新 API 适配（以编译通过为准）。**风险点**：0.7 可能连带提升 wasm-bindgen 要求，与 dioxus 0.8.0-alpha.1 锁的 wasm-bindgen 0.2.126 冲突——若 `dx build` 失败且无法兼容，回退 0.6 并在报告中说明（不阻塞其他修复）。

- [ ] **Step 2: combineds.rs checked → HashSet**

`FormState.checked: Vec<i64>` → `std::collections::HashSet<i64>`；`toggle_member` 用 `contains`/`remove`/`insert`（O(1)）；`open_create`/`open_edit` 构造处改 `HashSet::new()`/`c.source_ids.iter().copied().collect()`；member_rows 的 contains 照常。

- [ ] **Step 3: 构建验证 + Commit**

Run: `cd crates/server/web && dx build --web --release` 成功。

```bash
git add -A
git commit -m "chore(web): gloo-net 0.7, HashSet member selection
```
（提交信息结尾加 `Co-Authored-By: Claude <noreply@anthropic.com>`）

---

### Task 4: 全量验证与合并

**Files:** 无

- [ ] **Step 1: 全量验证**

Run（按顺序）: `cargo upgrade -i`、`cargo fmt --all`、`cargo clippy --workspace`、`cargo test --workspace`、`cd crates/server/web && dx build --web --release`、`make smoke`。
Expected: 全绿（cargo tree 确认无 serde_yaml/urlencoding/hex/gloo-net 0.6 残留）。

- [ ] **Step 2: 浏览器人工核对**

1. 订阅源/组合订阅/预览/配置页功能正常（前端重构后无回归）
2. 复制链接、成员勾选（HashSet 后交互正常）
3. `/subscribe/{name}` 输出正常
