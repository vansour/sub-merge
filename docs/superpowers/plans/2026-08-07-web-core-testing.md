# web-core 前端逻辑测试 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新建 `crates/web-core` 并把前端纯逻辑迁入、加入 workspace，使 `cargo test --workspace` 自动覆盖前端逻辑（映射函数 + DTO 契约测试），验证链与 CI 零改动。

**Architecture:** 前端纯逻辑（DTO、ApiError、映射函数）从 web crate 迁入新库 crate `submerge-web-core`（仅 serde+serde_json 依赖，host 原生可测），加入根 workspace members；`submerge-web` 通过 path 依赖引用。组件内 fetch/Signal/web-sys 调用全部留原地。

**Tech Stack:** Rust 2024、serde(derive) + serde_json、dioxus 0.8.0-alpha.1（仅 web crate，不动）。

## Global Constraints

- **web crate（crates/server/web）保持 workspace 之外**（dx build 与 cargo build 互不干扰的文档化约束）；验证链 `cargo upgrade -i / fmt --all / clippy --workspace / test --workspace` 不新增命令
- **web-core 仅依赖 `serde`(derive) + `serde_json`**，不得引入 dioxus/web-sys/wasm-bindgen/gloo-net
- **edition 2024**（`edition.workspace = true` 继承），rust-version 1.97（workspace 继承）
- 包名 **`submerge-web-core`**（依赖键/use 路径 = `submerge_web_core`），目录 `crates/web-core`
- **纯迁移，UI 行为零变化**；`kind_label` 是唯一新增函数（两处重复表达式的等价替换）；`subscribe_path` 不做 URL 编码（现状即不编码）
- **契约测试 fixture 必须与 server routes 实际输出形状一致**（本计划已从 `crates/server/src/routes/*.rs` 取样），不得臆造字段
- 新增 `#[allow(dead_code)]` 一律不需要（lib crate 的 pub 字段不会被 dead_code 标记）

---

### Task 1: web-core crate 脚手架 + workspace 接入

**Files:**
- Create: `crates/web-core/Cargo.toml`
- Create: `crates/web-core/src/lib.rs`
- Modify: `Cargo.toml`（根，members 数组）
- Modify: `crates/server/web/Cargo.toml`（dependencies 表）

**Interfaces:**
- Produces: workspace 新成员包 `submerge-web-core`（lib.rs 仅有 crate 级文档注释，模块由后续任务添加）

- [ ] **Step 1: 创建 `crates/web-core/Cargo.toml`**

```toml
[package]
name = "submerge-web-core"
version = "0.1.0"
# 前端纯逻辑库：无 dioxus/web-sys 依赖，host 原生可测。
# 加入根 workspace members，使 cargo test --workspace 自动覆盖前端逻辑。
edition.workspace = true

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

- [ ] **Step 2: 创建 `crates/web-core/src/lib.rs`**

```rust
//! submerge-web-core：sub-merge 前端纯逻辑（API 契约 DTO、错误类型、映射/格式化函数）。
//! 无 dioxus/web-sys 依赖，可在 host 原生目标直接 cargo test。
//! 依赖方向：submerge-web（web crate）→ 本 crate；本 crate 不依赖任何前端渲染代码。
```

- [ ] **Step 3: 根 `Cargo.toml` members 追加 web-core**

修改 `/root/github/sub-merge/Cargo.toml`：

```toml
members = ["crates/proxy-core", "crates/server", "crates/web-core"]
```

- [ ] **Step 4: web crate 添加 path 依赖**

修改 `/root/github/sub-merge/crates/server/web/Cargo.toml` 的 `[dependencies]`（`gloo-net` 行之后）：

```toml
submerge-web-core = { path = "../web-core" }  # 前端纯逻辑（DTO/错误/映射），host 可测
```

- [ ] **Step 5: 验证编译**

```bash
cd /root/github/sub-merge
cargo build --workspace        # 期望：web-core 编译成功，proxy-core/server 无回归
cd crates/server/web && cargo check   # 期望：通过（dep 暂未使用，cargo 不告警）
```

- [ ] **Step 6: Commit**

```bash
cd /root/github/sub-merge
git add Cargo.toml crates/web-core crates/server/web/Cargo.toml crates/server/web/Cargo.lock
git commit -m "feat(web-core): workspace 新成员 submerge-web-core 脚手架"
```

---

### Task 2: dto.rs + 契约测试 + web 组件 DTO 迁移

**Files:**
- Create: `crates/web-core/src/dto.rs`（测试 + 结构体）
- Modify: `crates/web-core/src/lib.rs`（加 `pub mod dto;`）
- Modify: `crates/server/web/src/components/sources.rs`、`combineds.rs`、`preview.rs`、`overview.rs`、`config.rs`（删本地 DTO 定义、改 import）
- Test: `crates/web-core/src/dto.rs` 内 `#[cfg(test)]`

**Interfaces:**
- Produces（后续任务与 web crate 依赖的精确签名）：
  - `submerge_web_core::dto::SourceDto` `{ pub id: i64, pub url: String, pub name: String, pub kind: String, pub enabled: bool, pub created_at: String }`，`#[derive(Debug, Clone, Deserialize)]`
  - `submerge_web_core::dto::CombinedDto` `{ pub id: i64, pub name: String, pub created_at: String, pub source_ids: Vec<i64> }`，`#[derive(Debug, Clone, Deserialize)]`
  - `submerge_web_core::dto::PreviewNode` `{ pub name: String, pub protocol: String, pub server: String, pub port: u16 }`，`#[derive(Debug, Clone, Deserialize)]`
  - `submerge_web_core::dto::PreviewResp` `{ pub nodes: Vec<PreviewNode>, pub errors: Vec<String>, pub total: usize }`，`#[derive(Debug, Clone, Deserialize)]`
  - `submerge_web_core::dto::PreviewSummary` `{ pub total: usize, pub errors: Vec<String> }`，`#[derive(Debug, Clone, Deserialize)]`
  - `submerge_web_core::dto::ConfigDto` `{ pub admin_token: String }`，`#[derive(Debug, Clone, Deserialize)]`

- [ ] **Step 1: 写失败测试 —— 创建 `crates/web-core/src/dto.rs`（仅测试，无结构体）**

`crates/web-core/src/lib.rs` 追加 `pub mod dto;`。`dto.rs` 内容（结构体尚不存在，此时故意编译失败）：

```rust
// 与 server 的 API 契约 DTO。fixture 取自 crates/server/src/routes/*.rs 实际输出形状。
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_dto_parses_full_fields() {
        let j = r#"{"id":1,"url":"https://example.com/sub","name":"机场A","kind":"remote","enabled":true,"created_at":"2026-08-07 12:00:00"}"#;
        let d: SourceDto = serde_json::from_str(j).unwrap();
        assert_eq!(d.id, 1);
        assert_eq!(d.url, "https://example.com/sub");
        assert_eq!(d.name, "机场A");
        assert_eq!(d.kind, "remote");
        assert!(d.enabled);
        assert_eq!(d.created_at, "2026-08-07 12:00:00");
    }

    #[test]
    fn source_dto_single_kind() {
        let j = r#"{"id":2,"url":"ss://a@b:8388","name":"单条","kind":"single","enabled":false,"created_at":"2026-08-07 12:00:00"}"#;
        let d: SourceDto = serde_json::from_str(j).unwrap();
        assert_eq!(d.kind, "single");
        assert!(!d.enabled);
    }

    #[test]
    fn combined_dto_parses_source_ids() {
        let j = r#"{"id":1,"name":"home","created_at":"2026-08-07 12:00:00","source_ids":[1,2]}"#;
        let d: CombinedDto = serde_json::from_str(j).unwrap();
        assert_eq!(d.id, 1);
        assert_eq!(d.name, "home");
        assert_eq!(d.source_ids, vec![1, 2]);
    }

    #[test]
    fn preview_resp_parses_full_shape() {
        let j = r#"{"nodes":[{"name":"节点1","protocol":"vmess","server":"1.2.3.4","port":443}],"errors":["源A: 超时"],"total":1}"#;
        let d: PreviewResp = serde_json::from_str(j).unwrap();
        assert_eq!(d.nodes.len(), 1);
        assert_eq!(d.nodes[0].name, "节点1");
        assert_eq!(d.nodes[0].protocol, "vmess");
        assert_eq!(d.nodes[0].server, "1.2.3.4");
        assert_eq!(d.nodes[0].port, 443);
        assert_eq!(d.errors, vec!["源A: 超时"]);
        assert_eq!(d.total, 1);
    }

    #[test]
    fn preview_resp_empty() {
        let j = r#"{"nodes":[],"errors":[],"total":0}"#;
        let d: PreviewResp = serde_json::from_str(j).unwrap();
        assert!(d.nodes.is_empty());
        assert!(d.errors.is_empty());
        assert_eq!(d.total, 0);
    }

    #[test]
    fn preview_summary_parses() {
        let j = r#"{"total":10,"errors":["a"]}"#;
        let d: PreviewSummary = serde_json::from_str(j).unwrap();
        assert_eq!(d.total, 10);
        assert_eq!(d.errors, vec!["a"]);
    }

    #[test]
    fn config_dto_parses() {
        let j = r#"{"admin_token":"abc123"}"#;
        let d: ConfigDto = serde_json::from_str(j).unwrap();
        assert_eq!(d.admin_token, "abc123");
    }

    #[test]
    fn unknown_fields_ignored() {
        let j = r#"{"id":1,"url":"x","name":"n","kind":"remote","enabled":true,"created_at":"t","extra":"ignored"}"#;
        let d: SourceDto = serde_json::from_str(j).unwrap();
        assert_eq!(d.name, "n");
    }
}
```

- [ ] **Step 2: 运行确认失败**

```bash
cd /root/github/sub-merge
cargo test -p submerge-web-core
```
期望：编译失败，报 `unresolved import` / 找不到 `SourceDto` 等（结构体未定义）。

- [ ] **Step 3: 写实现 —— 在 `dto.rs` 测试上方追加结构体定义**

```rust
// 与 server 的 API 契约 DTO。fixture 取自 crates/server/src/routes/*.rs 实际输出形状。
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct SourceDto {
    pub id: i64,
    pub url: String,
    pub name: String,
    pub kind: String,
    pub enabled: bool,
    // 后端返回的字段，作为 API 契约保留；UI 暂不展示。
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CombinedDto {
    pub id: i64,
    pub name: String,
    // 后端返回的字段，作为 API 契约保留；UI 暂不展示。
    pub created_at: String,
    pub source_ids: Vec<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PreviewNode {
    pub name: String,
    pub protocol: String,
    pub server: String,
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PreviewResp {
    pub nodes: Vec<PreviewNode>,
    pub errors: Vec<String>,
    pub total: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PreviewSummary {
    pub total: usize,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConfigDto {
    pub admin_token: String,
}

#[cfg(test)]
mod tests {
    // …… Step 1 的测试代码原样保留
}
```

- [ ] **Step 4: 运行确认通过**

```bash
cargo test -p submerge-web-core
```
期望：9 个测试全部 PASS。

- [ ] **Step 5: web crate 组件迁移 DTO 引用（5 处）**

逐个修改（删本地定义 → 加 import）：

`sources.rs`：删除 `struct SourceDto`（第 10-20 行含 `#[allow(dead_code)]` 的定义），文件头加：
```rust
use submerge_web_core::dto::SourceDto;
```

`combineds.rs`：删除 `struct CombinedDto`（第 12-20 行），文件头加：
```rust
use submerge_web_core::dto::CombinedDto;
```

`preview.rs`：删除 `struct PreviewNode` 与 `struct PreviewResp`（第 10-23 行），文件头加：
```rust
use submerge_web_core::dto::PreviewResp;
```

`overview.rs`：删除 `struct PreviewSummary`（第 10-15 行），文件头加：
```rust
use submerge_web_core::dto::PreviewSummary;
```

`config.rs`：删除 `struct ConfigDto`（第 11-14 行），文件头加：
```rust
use submerge_web_core::dto::ConfigDto;
```

注意：preview.rs 的 `PreviewNode` 类型在迁移后不再被组件直接命名（只出现在 `PreviewResp` 内部），无需 import；`serde::Deserialize` import 若不再被使用则一并删除（preview.rs/overview.rs/config.rs 顶部 `use serde::Deserialize;` 会变成未使用，删掉；sources.rs/combineds.rs 的 `use serde::Deserialize;` 同样处理——它们没有其他 serde 派生类型了）。

- [ ] **Step 6: 验证 web crate 编译**

```bash
cd /root/github/sub-merge/crates/server/web && cargo check
```
期望：编译通过，无 unused import 警告（若有，删除残留的 `use serde::Deserialize;`）。

- [ ] **Step 7: Commit**

```bash
cd /root/github/sub-merge
git add crates/web-core crates/server/web/src
git commit -m "feat(web-core): DTO 迁入 web-core + server 契约反序列化测试"
```

---

### Task 3: error.rs + 测试 + api.rs 迁移

**Files:**
- Create: `crates/web-core/src/error.rs`
- Modify: `crates/web-core/src/lib.rs`（`pub mod error;`）
- Modify: `crates/server/web/src/api.rs`（删 ApiError 定义、加 import）
- Test: `crates/web-core/src/error.rs` 内 `#[cfg(test)]`

**Interfaces:**
- Produces: `submerge_web_core::error::ApiError` `{ pub status: Option<u16>, pub message: String }`，实现 `Debug`、`Display`（输出 message）、`From<ApiError> for String`
- Consumes: 无

- [ ] **Step 1: 写失败测试 —— 创建 `crates/web-core/src/error.rs`（仅测试）**

`lib.rs` 追加 `pub mod error;`。`error.rs`：

```rust
// API 请求错误类型：区分 HTTP 状态码与网络/解析错误。
// status = Some(401) 表示鉴权失败（token 失效），调用方据此决定是否清除本地 token；
// 网络错误/5xx 等瞬时错误 status 为 None 或非 401，不得清除 token。
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_prints_message() {
        let e = ApiError { status: Some(401), message: "HTTP 401: unauthorized".into() };
        assert_eq!(e.to_string(), "HTTP 401: unauthorized");
    }

    #[test]
    fn display_without_status() {
        let e = ApiError { status: None, message: "network down".into() };
        assert_eq!(e.to_string(), "network down");
    }

    #[test]
    fn into_string_converts() {
        let e = ApiError { status: None, message: "boom".into() };
        let s: String = e.into();
        assert_eq!(s, "boom");
    }

    #[test]
    fn status_field_semantics() {
        // 401 与瞬时错误（None/5xx）的区分由调用方决定，这里锁定字段语义防回归。
        assert_eq!(ApiError { status: Some(401), message: "".into() }.status, Some(401));
        assert_eq!(ApiError { status: Some(500), message: "".into() }.status, Some(500));
        assert_eq!(ApiError { status: None, message: "".into() }.status, None);
    }
}
```

- [ ] **Step 2: 运行确认失败**

```bash
cargo test -p submerge-web-core
```
期望：编译失败（`ApiError` 未定义）。

- [ ] **Step 3: 写实现 —— 测试上方追加（原样搬自 api.rs 第 8-24 行）**

```rust
// API 请求错误类型：区分 HTTP 状态码与网络/解析错误。
// status = Some(401) 表示鉴权失败（token 失效），调用方据此决定是否清除本地 token；
// 网络错误/5xx 等瞬时错误 status 为 None 或非 401，不得清除 token。
#[derive(Debug)]
pub struct ApiError {
    pub status: Option<u16>,
    pub message: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl From<ApiError> for String {
    fn from(e: ApiError) -> String {
        e.message
    }
}

#[cfg(test)]
mod tests {
    // …… Step 1 的测试代码原样保留
}
```

- [ ] **Step 4: 运行确认通过**

```bash
cargo test -p submerge-web-core
```
期望：13 个测试全部 PASS。

- [ ] **Step 5: 修改 `crates/server/web/src/api.rs`**

删除第 5-24 行（`ApiError` 结构体 + `Display` + `From<String>` 三个 impl），文件头（`use std::str::FromStr;` 附近）加：

```rust
use submerge_web_core::error::ApiError;
```

- [ ] **Step 6: 验证 web crate 编译**

```bash
cd /root/github/sub-merge/crates/server/web && cargo check
```
期望：编译通过。

- [ ] **Step 7: Commit**

```bash
cd /root/github/sub-merge
git add crates/web-core crates/server/web/src
git commit -m "feat(web-core): ApiError 迁入 web-core + 错误语义测试"
```

---

### Task 4: fmt.rs + 测试 + 组件映射迁移

**Files:**
- Create: `crates/web-core/src/fmt.rs`
- Modify: `crates/web-core/src/lib.rs`（`pub mod fmt;`）
- Modify: `crates/server/web/src/components/toast.rs`、`preview.rs`、`config.rs`、`sources.rs`、`combineds.rs`
- Test: `crates/web-core/src/fmt.rs` 内 `#[cfg(test)]`

**Interfaces:**
- Produces（web crate 后续引用）：
  - `submerge_web_core::fmt::ToastKind`：`#[derive(Debug, Clone, Copy, PartialEq)]` 枚举 `Success | Error | Info`
  - `submerge_web_core::fmt::toast_icon(kind: ToastKind) -> &'static str`：Success→"check"，Error→"alert"，Info→"config"
  - `submerge_web_core::fmt::toast_class(kind: ToastKind) -> &'static str`：Success→"success"，Error→"error"，Info→"info"
  - `submerge_web_core::fmt::proto_class(protocol: &str) -> &'static str`：ss/ssr→"proto-0"，vmess/vless→"proto-1"，trojan→"proto-2"，hysteria/hysteria2→"proto-3"，tuic→"proto-4"，其他→"proto-5"
  - `submerge_web_core::fmt::kind_label(kind: &str) -> &'static str`：single→"单条"，其他→"远程"
  - `submerge_web_core::fmt::mask_token(_: &str) -> &'static str`：恒返回 "••••••••"
  - `submerge_web_core::fmt::subscribe_path(name: &str, fmt: &str) -> String`：`format!("/subscribe/{name}?format={fmt}")`，不编码
  - `submerge_web_core::fmt::next_toast_id() -> u64`：thread_local 自增，从 1 开始

- [ ] **Step 1: 写失败测试 —— 创建 `crates/web-core/src/fmt.rs`（仅测试）**

`lib.rs` 追加 `pub mod fmt;`。`fmt.rs`：

```rust
// 前端展示映射/格式化纯函数（协议配色、类型文案、token 掩码、订阅链接路径、toast 映射与 id 分配）。
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proto_class_known_protocols() {
        assert_eq!(proto_class("ss"), "proto-0");
        assert_eq!(proto_class("ssr"), "proto-0");
        assert_eq!(proto_class("vmess"), "proto-1");
        assert_eq!(proto_class("vless"), "proto-1");
        assert_eq!(proto_class("trojan"), "proto-2");
        assert_eq!(proto_class("hysteria"), "proto-3");
        assert_eq!(proto_class("hysteria2"), "proto-3");
        assert_eq!(proto_class("tuic"), "proto-4");
    }

    #[test]
    fn proto_class_fallback() {
        assert_eq!(proto_class("wireguard"), "proto-5");
        assert_eq!(proto_class(""), "proto-5");
    }

    #[test]
    fn kind_label_branches() {
        assert_eq!(kind_label("single"), "单条");
        assert_eq!(kind_label("remote"), "远程");
        assert_eq!(kind_label("unknown"), "远程"); // 非 single 一律远程（与现状一致）
    }

    #[test]
    fn mask_token_hides_any_token() {
        assert_eq!(mask_token("abc"), "••••••••");
        assert_eq!(mask_token(&"x".repeat(100)), "••••••••");
        assert_eq!(mask_token(""), "••••••••");
    }

    #[test]
    fn subscribe_path_builds() {
        assert_eq!(subscribe_path("home", "clash"), "/subscribe/home?format=clash");
        assert_eq!(subscribe_path("home", "v2ray"), "/subscribe/home?format=v2ray");
        assert_eq!(subscribe_path("home", "singbox"), "/subscribe/home?format=singbox");
        // 现状不做 URL 编码：含特殊字符时原样拼接。
        assert_eq!(subscribe_path("my sub", "clash"), "/subscribe/my sub?format=clash");
    }

    #[test]
    fn toast_mappings() {
        assert_eq!(toast_icon(ToastKind::Success), "check");
        assert_eq!(toast_icon(ToastKind::Error), "alert");
        assert_eq!(toast_icon(ToastKind::Info), "config");
        assert_eq!(toast_class(ToastKind::Success), "success");
        assert_eq!(toast_class(ToastKind::Error), "error");
        assert_eq!(toast_class(ToastKind::Info), "info");
    }

    #[test]
    fn toast_ids_monotonic() {
        let a = next_toast_id();
        let b = next_toast_id();
        assert_eq!(b, a + 1);
    }
}
```

- [ ] **Step 2: 运行确认失败**

```bash
cargo test -p submerge-web-core
```
期望：编译失败（函数/枚举未定义）。

- [ ] **Step 3: 写实现 —— 测试上方追加**

```rust
// 前端展示映射/格式化纯函数（协议配色、类型文案、token 掩码、订阅链接路径、toast 映射与 id 分配）。

/// Toast 种类。文案由 push_toast 调用方提供，这里只负责图标与样式映射。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ToastKind {
    Success,
    Error,
    Info,
}

/// Toast 图标名（icon.rs 的 SVG 名）。
pub fn toast_icon(kind: ToastKind) -> &'static str {
    match kind {
        ToastKind::Success => "check",
        ToastKind::Error => "alert",
        ToastKind::Info => "config",
    }
}

/// Toast 样式类（index.html 的 .toast.success/.error/.info）。
pub fn toast_class(kind: ToastKind) -> &'static str {
    match kind {
        ToastKind::Success => "success",
        ToastKind::Error => "error",
        ToastKind::Info => "info",
    }
}

/// 协议 → 配色（CSS --proto-0..5）。同族协议同色。
pub fn proto_class(protocol: &str) -> &'static str {
    match protocol {
        "ss" | "ssr" => "proto-0",
        "vmess" | "vless" => "proto-1",
        "trojan" => "proto-2",
        "hysteria" | "hysteria2" => "proto-3",
        "tuic" => "proto-4",
        _ => "proto-5",
    }
}

/// 订阅源类型 → 展示文案（单条/远程）。
pub fn kind_label(kind: &str) -> &'static str {
    if kind == "single" { "单条" } else { "远程" }
}

/// 管理 token 掩码展示：任何 token 一律显示固定 8 个 •。
pub fn mask_token(_: &str) -> &'static str {
    "••••••••"
}

/// 组合订阅输出路径（不含 base origin，origin 由组件从 window.location 拼装）。
/// 与现状一致不做 URL 编码。
pub fn subscribe_path(name: &str, fmt: &str) -> String {
    format!("/subscribe/{name}?format={fmt}")
}

thread_local! {
    static NEXT_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
}

/// 分配自增 toast id（从 1 开始）。
pub fn next_toast_id() -> u64 {
    NEXT_ID.with(|c| {
        let v = c.get();
        c.set(v + 1);
        v
    })
}

#[cfg(test)]
mod tests {
    // …… Step 1 的测试代码原样保留
}
```

- [ ] **Step 4: 运行确认通过**

```bash
cargo test -p submerge-web-core
```
期望：20 个测试全部 PASS（dto 9 + error 4 + fmt 7）。

- [ ] **Step 5: web crate 组件迁移（5 个文件）**

**toast.rs**：
- 删除第 9-14 行 `ToastKind` 枚举定义与第 23-25 行 `NEXT_ID` thread_local
- 文件头 `use wasm_bindgen::prelude::*;` 附近加：
```rust
use submerge_web_core::fmt::{next_toast_id, toast_class, toast_icon};
pub use submerge_web_core::fmt::ToastKind;  // 保持既有 `use crate::components::toast::ToastKind` 路径可用
```
- `push_toast`（第 28-35 行）函数体换成：
```rust
pub fn push_toast(mut toasts: Signal<Vec<ToastMsg>>, kind: ToastKind, text: impl Into<String>) {
    let id = next_toast_id();
    toasts.write().push(ToastMsg { id, kind, text: text.into() });
}
```
- `ToastCard` 内两个 match（第 82-91 行 `icon_name` 与 `kind_class`）换成：
```rust
    let icon_name = toast_icon(kind);
    let kind_class = toast_class(kind);
```

**preview.rs**：删除 `fn proto_class`（第 25-35 行），文件头加：
```rust
use submerge_web_core::fmt::proto_class;
```

**config.rs**：`admin_token_show`（第 89-99 行）的 else 分支 `"••••••••".to_string()` 换成：
```rust
mask_token(&c.admin_token).to_string()
```
文件头加 `use submerge_web_core::fmt::mask_token;`

**sources.rs**：徽章文案 `if kind == "single" { "单条" } else { "远程" }`（第 203 行）换成 `{kind_label(&kind)}`；文件头加 `use submerge_web_core::fmt::kind_label;`。注意 class 三元 `if kind == "single" { "info" } else { "off" }` **不动**（那是徽章配色，不是文案）。

**combineds.rs**：
- 第 263 行 `let link = format!("{}/subscribe/{}?format={}", base, name, fmt);` 换成：
```rust
let link = format!("{}{}", base, subscribe_path(&name, fmt));
```
- 第 235-236 行徽章文案 `if kind == "single" { "单条" } else { "远程" }` 换成 `{kind_label(&kind)}`（class 三元不动）
- 文件头加：
```rust
use submerge_web_core::fmt::{kind_label, subscribe_path};
```
- **ToastKind import 一律不动**：toast.rs 已 re-export `pub use submerge_web_core::fmt::ToastKind;`，combineds.rs/sources.rs/config.rs 现有的 `use crate::components::toast::ToastKind`（或从 toast.rs 导入 ToastKind 的写法）路径不变、继续有效

- [ ] **Step 6: 验证 web crate 编译**

```bash
cd /root/github/sub-merge/crates/server/web && cargo check
```
期望：编译通过，无 unused import / unused variable 警告。

- [ ] **Step 7: 全量验证**

```bash
cd /root/github/sub-merge
cargo test -p submerge-web-core
cargo clippy --workspace
cargo fmt --all -- --check   # 若报格式差异，先 cargo fmt --all 再重跑
```
期望：全部通过。

- [ ] **Step 8: Commit**

```bash
git add crates/web-core crates/server/web/src
git commit -m "feat(web-core): 映射函数迁入 web-core + 组件引用迁移"
```

---

### Task 5: 全链验证 + 文档

**Files:**
- Modify: `CLAUDE.md`（架构节补 web-core 说明）
- 其他：验证命令运行（不产生源码改动；`crates/server/web/Cargo.lock` 若被 dx build 更新则一并提交）

- [ ] **Step 1: 跑完整强制验证链（CLAUDE.md 顺序）**

```bash
cd /root/github/sub-merge
cargo upgrade -i
cargo fmt --all
cargo clippy --workspace
cargo test --workspace
```
期望：全部通过；`cargo test --workspace` 输出中可见 `submerge-web-core` 测试（`Running unittests src/lib.rs` + 20 个测试）。

- [ ] **Step 2: 前端构建与端到端冒烟**

```bash
cd /root/github/sub-merge/crates/server/web && dx build --web --release
cd /root/github/sub-merge && make smoke
```
期望：dx build 成功（web-core 被连带编译为 wasm）；smoke 脚本全部通过（SPA/静态/API 无回归）。

- [ ] **Step 3: 更新 CLAUDE.md 架构节**

在 `crates/server/web` 条目后追加：

```markdown
- `crates/web-core`（submerge-web-core）：前端纯逻辑库（API 契约 DTO、ApiError、映射/格式化函数），**在 workspace 内**，由 `cargo test --workspace` 覆盖；无 dioxus/web-sys 依赖，host 原生可测。web crate 以 path 依赖引用。
```

- [ ] **Step 4: Commit**

```bash
git add CLAUDE.md crates/server/web/Cargo.lock
git commit -m "docs: CLAUDE.md 补充 web-core；web crate lock 随新依赖更新"
```

- [ ] **Step 5: 核对完成判定（spec 清单）**

```bash
cargo test -p submerge-web-core -- --list | grep -c "test"   # 期望 20
cargo test --workspace                                       # 期望全绿
cd crates/server/web && dx build --web --release             # 期望成功
make smoke                                                   # 期望全绿
```
CLAUDE.md 验证链命令未增删（`cargo upgrade -i / fmt --all / clippy --workspace / test --workspace` 与改动前一致）。
