# Plan C: Dioxus 前端 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 构建 Dioxus Web (WASM) 管理界面：登录（输入管理 token）、订阅源列表 CRUD、转换结果预览、订阅链接复制、token 轮换。产出静态 WASM 资源到 `crates/server/web/dist`，由 axum 托管。

**Architecture:** Dioxus 0.8 Web (WASM) 模式，编译为 `wasm32-unknown-unknown`，静态资源由 Plan B 的 axum 服务托管。前端纯客户端渲染，通过 `fetch` 调 `/api/admin/*`，管理 token 存 `localStorage`。

**Tech Stack:** dioxus 0.8 (web), wasm-bindgen, serde/serde_json, reqwest (wasm) 或 dioxus fetch。构建工具：`dx`（Dioxus CLI）。

## Global Constraints

- 前端目录：`crates/server/web/`，源码 `crates/server/web/src/main.rs`
- 构建产物：`crates/server/web/dist/`（axum `WEB_DIST` 指向此处，默认 `./web/dist`）
- 目标平台：`wasm32-unknown-unknown`
- 管理 token 从输入框输入，存 `localStorage`，每次请求带 `Authorization: Bearer`
- 不包含任何订阅 token 展示给未授权用户；配置页需登录后可见
- 交互：订阅源增删改、启用/禁用、手动刷新、预览节点列表、复制订阅链接、token 轮换
- UI 语言：中文
- 依赖尽量少，避免 Dioxus 0.8 alpha 的未稳定 API

## 文件结构总览

```
crates/server/web/
├── Cargo.toml
├── index.html
├── dx.toml            # Dioxus 构建配置
└── src/
    ├── main.rs        # 入口 + 根组件 + 路由（简单状态切换）
    ├── api.rs         # API 客户端（fetch 封装）
    ├── components/
    │   ├── mod.rs
    │   ├── login.rs   # 登录页
    │   ├── sources.rs # 订阅源管理
    │   ├── preview.rs # 预览
    │   └── config.rs  # token 管理 + 订阅链接复制
```

---

### Task 1: 前端脚手架 + 构建管线

**Files:**
- Create: `/root/github/sub-merge/crates/server/web/Cargo.toml`
- Create: `/root/github/sub-merge/crates/server/web/index.html`
- Create: `/root/github/sub-merge/crates/server/web/dx.toml`
- Create: `/root/github/sub-merge/crates/server/web/src/main.rs`（最小可渲染）
- Create: `/root/github/sub-merge/crates/server/web/src/api.rs`（空骨架）

**Interfaces:**
- Consumes: 无
- Produces:
  - `fn App() -> Element` — 根组件，先渲染静态文本
  - `api.rs`: `pub async fn request(method: &str, path: &str, body: Option<String>, token: Option<&str>) -> Result<String, String>` — 基础 fetch 封装

- [ ] **Step 1: 安装 WASM target 与 dx 工具（若未安装）**

```bash
rustup target add wasm32-unknown-unknown
cargo install dioxus-cli  # 可能较慢；或使用 dx 已装版本
```

- [ ] **Step 2: 创建 web/Cargo.toml**

```toml
[package]
name = "submerge-web"
version = "0.1.0"
edition.workspace = true

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
dioxus = { version = "0.8", features = ["web"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
wasm-bindgen = "0.2"
wasm-bindgen-futures = "0.4"
js-sys = "0.3"
web-sys = { version = "0.3", features = ["console", "window", "Storage", "localStorage"] }
gloo-net = { version = "0.6", features = ["http"] }  # 轻量 fetch 封装

[profile.release]
opt-level = "s"
```

> **说明**：0.8 alpha 的 `dioxus/web` feature 命名以实际发布为准。若 `dioxus = { version = "0.8", features = ["web"] }` 不可用，回退到 `dioxus-web` crate + `dioxus::launch`（见 Step 5 的备选）。

- [ ] **Step 3: 创建 index.html**

```html
<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>sub-merge 管理</title>
  <style>
    body { font-family: system-ui, -apple-system, sans-serif; margin: 0; padding: 0; background: #f5f5f7; color: #1d1d1f; }
    .container { max-width: 900px; margin: 0 auto; padding: 20px; }
    .card { background: #fff; border-radius: 10px; padding: 16px; margin-bottom: 16px; box-shadow: 0 1px 3px rgba(0,0,0,0.1); }
    button { background: #0071e3; color: #fff; border: none; padding: 8px 14px; border-radius: 6px; cursor: pointer; font-size: 14px; }
    button.secondary { background: #e8e8ed; color: #1d1d1f; }
    button.danger { background: #ff3b30; }
    input { width: 100%; padding: 8px; margin: 6px 0; border: 1px solid #d1d1d6; border-radius: 6px; font-size: 14px; box-sizing: border-box; }
    table { width: 100%; border-collapse: collapse; }
    th, td { text-align: left; padding: 8px; border-bottom: 1px solid #e5e5ea; font-size: 14px; }
    .badge { padding: 2px 8px; border-radius: 10px; font-size: 12px; }
    .badge.on { background: #e8f7ee; color: #1a7f37; }
    .badge.off { background: #fbe9e9; color: #b42318; }
  </style>
</head>
<body>
  <div id="root"></div>
  <script src="dist/index.js"></script>
</body>
</html>
```

- [ ] **Step 4: 创建 dx.toml**

```toml
[web]
out_dir = "dist"
```

- [ ] **Step 5: 创建最小 main.rs**

```rust
// crates/server/web/src/main.rs
use dioxus::prelude::*;

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    rsx! {
        div { class: "container",
            h1 { "sub-merge 管理" }
            p { "界面待完善" }
        }
    }
}
```

- [ ] **Step 6: 创建 api.rs 骨架**

```rust
// crates/server/web/src/api.rs
use gloo_net::http::Request;

/// 基础 fetch 封装。返回 body 字符串或错误。
pub async fn request(
    method: &str,
    path: &str,
    body: Option<&str>,
    token: Option<&str>,
) -> Result<String, String> {
    let mut req = Request::new(path).method(method);
    if let Some(t) = token {
        req = req.header("Authorization", &format!("Bearer {}", t));
    }
    if let Some(b) = body {
        req = req.header("Content-Type", "application/json").body(b);
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    if status >= 200 && status < 300 {
        Ok(text)
    } else {
        Err(format!("HTTP {status}: {text}"))
    }
}
```

- [ ] **Step 7: 构建验证**

```bash
cd /root/github/sub-merge/crates/server/web
dx build --web  # 或 dx build
ls -la dist/    # 应产出 index.js 等
```

- [ ] **Step 8: 运行确认通过**

Run: `cd /root/github/sub-merge/crates/server/web && dx build --web`
Expected: 成功产出 `dist/`，无编译错误

- [ ] **Step 9: Commit**

```bash
git add crates/server/web/
git commit -m "feat(web): dioxus scaffold, build pipeline, api client skeleton"
```

---

### Task 2: 登录页 + 会话管理（localStorage）

**Files:**
- Create: `crates/server/web/src/components/mod.rs`
- Create: `crates/server/web/src/components/login.rs`
- Modify: `crates/server/web/src/main.rs`（接入登录状态）

**Interfaces:**
- Consumes: `api::request`
- Produces:
  - `fn Login(admin_token: Signal<String>) -> Element` — 输入 token，验证后写 localStorage
  - `fn read_token() -> String` / `fn write_token(t: &str)` / `fn clear_token()`
  - 主组件在 token 为空时显示 Login，有 token 时显示主界面

- [ ] **Step 1: 先确定状态模型**

主组件持有 `admin_token: Signal<Option<String>>`：
```rust
#[component]
fn App() -> Element {
    let token = use_signal(|| read_token());
    rsx! {
        div { class: "container",
            match token().as_deref() {
                // Task 2 先用占位；Task 3 替换为 MainShell
                Some(_) => rsx! {
                    div {
                        h1 { "sub-merge 管理" }
                        p { "已登录（主界面 Task 3 实现）" }
                    }
                },
                None => rsx! { Login { on_login: move |t| {
                    write_token(&t);
                    token.set(Some(t));
                } } },
            }
        }
    }
}
```

- [ ] **Step 2: 实现 components/mod.rs**

```rust
// crates/server/web/src/components/mod.rs
pub mod config;
pub mod login;
pub mod preview;
pub mod sources;
```

- [ ] **Step 3: 实现 login.rs**

```rust
// crates/server/web/src/components/login.rs
use crate::api::request;
use dioxus::prelude::*;

pub fn read_token() -> Option<String> {
    let w = web_sys::window()?;
    let s = w.local_storage().ok().flatten()?;
    s.get_item("submerge_admin_token").ok().flatten()
}

pub fn write_token(t: &str) {
    if let Ok(Some(s)) = web_sys::window().and_then(|w| w.local_storage()) {
        let _ = s.set_item("submerge_admin_token", t);
    }
}

pub fn clear_token() {
    if let Ok(Some(s)) = web_sys::window().and_then(|w| w.local_storage()) {
        let _ = s.remove_item("submerge_admin_token");
    }
}

#[component]
pub fn Login(on_login: EventHandler<String>) -> Element {
    let mut input = use_signal(String::new);
    let mut error = use_signal(String::new);
    let mut loading = use_signal(|| false);

    let submit = move |_| {
        if input.read().is_empty() {
            return;
        }
        let token = input.read().clone();
        loading.set(true);
        spawn(async move {
            // 用 GET /api/admin/config 验证 token 有效性
            match request("GET", "/api/admin/config", None, Some(&token)).await {
                Ok(_) => on_login.call(token),
                Err(e) => error.set(format!("登录失败: {}", e)),
            }
            loading.set(false);
        });
    };

    rsx! {
        div { class: "container",
            div { class: "card",
                h2 { "登录" }
                p { "请输入管理 token 进入管理界面" }
                input {
                    placeholder: "管理 token",
                    value: input,
                    oninput: move |e| input.set(e.value()),
                }
                if !error.read().is_empty() {
                    p { style: "color: #ff3b30", "{error}" }
                }
                button { onclick: submit, disabled: *loading.read(), "登录" }
            }
        }
    }
}
```

- [ ] **Step 4: 更新 main.rs 接入 Login/Main**

```rust
// crates/server/web/src/main.rs
mod api;
mod components;

use components::login::{read_token, write_token, Login};
use dioxus::prelude::*;

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    let token = use_signal(|| read_token());
    rsx! {
        div {
            match token().as_deref() {
                // Task 2 占位；Task 3 替换为 components::main_shell::MainShell
                Some(_) => rsx! {
                    div { class: "container",
                        h1 { "sub-merge 管理" }
                        p { "已登录（主界面 Task 3 实现）" }
                    }
                },
                None => rsx! {
                    Login {
                        on_login: move |t| {
                            write_token(&t);
                            token.set(Some(t));
                        },
                    }
                },
            }
        }
    }
}
```

> **说明**：`MainShell` 组件在 Task 3 实现（顶部导航 + 三块 Tab）。Task 2 先渲染占位 div，保证编译通过。

- [ ] **Step 5: 构建验证**

Run: `cd crates/server/web && dx build --web`
Expected: 成功

- [ ] **Step 6: Commit**

```bash
git add crates/server/web/
git commit -m "feat(web): login page with localStorage session"
```

---

### Task 3: 主界面骨架 + 订阅源管理

**Files:**
- Create: `crates/server/web/src/components/sources.rs`
- Modify: `crates/server/web/src/main.rs`（主界面 tab 导航）

**Interfaces:**
- Consumes: `api::request`, `admin_token: Signal<Option<String>>`
- Produces:
  - `#[derive(Deserialize)] pub struct SourceDto { id: i64, url: String, name: String, enabled: bool, created_at: String }`
  - `pub fn Sources(token: Signal<Option<String>>) -> Element`
  - 操作：list、create（url+name）、toggle enabled、delete、refresh

- [ ] **Step 1: 实现 sources.rs**

```rust
// crates/server/web/src/components/sources.rs
use crate::api::request;
use dioxus::prelude::*;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct SourceDto {
    pub id: i64,
    pub url: String,
    pub name: String,
    pub enabled: bool,
    pub created_at: String,
}

#[component]
pub fn Sources(token: Signal<Option<String>>) -> Element {
    let mut sources = use_signal(Vec::<SourceDto>::new);
    let mut error = use_signal(String::new);
    let mut new_url = use_signal(String::new);
    let mut new_name = use_signal(String::new);
    let mut loading = use_signal(|| false);

    // 加载列表
    let load = {
        let token = token.read().clone();
        let mut sources = sources.clone();
        spawn(async move {
            match request("GET", "/api/admin/sources", None, token.as_deref()).await {
                Ok(body) => {
                    if let Ok(list) = serde_json::from_str::<Vec<SourceDto>>(&body) {
                        sources.set(list);
                    }
                }
                Err(e) => error.set(e),
            }
        });
    };
    let _ = load;

    let add = move |_| {
        let url = new_url.read().clone();
        let name = new_name.read().clone();
        if url.is_empty() || name.is_empty() {
            error.set("URL 和名称不能为空".into());
            return;
        }
        let token = token.read().clone();
        let body = serde_json::json!({ "url": url, "name": name }).to_string();
        let mut sources = sources.clone();
        spawn(async move {
            match request("POST", "/api/admin/sources", Some(&body), token.as_deref()).await {
                Ok(_) => {
                    // 重新加载
                    if let Ok(body) = request("GET", "/api/admin/sources", None, token.as_deref()).await {
                        if let Ok(list) = serde_json::from_str::<Vec<SourceDto>>(&body) {
                            sources.set(list);
                        }
                    }
                    new_url.set(String::new());
                    new_name.set(String::new());
                }
                Err(e) => error.set(e),
            }
        });
    };

    let toggle = move |id: i64, enabled: bool| {
        let token = token.read().clone();
        let body = serde_json::json!({ "enabled": !enabled }).to_string();
        let mut sources = sources.clone();
        spawn(async move {
            let _ = request("PUT", &format!("/api/admin/sources/{}", id), Some(&body), token.as_deref()).await;
            if let Ok(body) = request("GET", "/api/admin/sources", None, token.as_deref()).await {
                if let Ok(list) = serde_json::from_str::<Vec<SourceDto>>(&body) {
                    sources.set(list);
                }
            }
        });
    };

    let del = move |id: i64| {
        let token = token.read().clone();
        let mut sources = sources.clone();
        spawn(async move {
            let _ = request("DELETE", &format!("/api/admin/sources/{}", id), None, token.as_deref()).await;
            if let Ok(body) = request("GET", "/api/admin/sources", None, token.as_deref()).await {
                if let Ok(list) = serde_json::from_str::<Vec<SourceDto>>(&body) {
                    sources.set(list);
                }
            }
        });
    };

    let refresh = move |id: i64| {
        let token = token.read().clone();
        spawn(async move {
            let _ = request("POST", &format!("/api/admin/sources/{}/refresh", id), None, token.as_deref()).await;
        });
    };

    rsx! {
        div { class: "card",
            h2 { "订阅源" }
            if !error.read().is_empty() {
                p { style: "color: #ff3b30", "{error}" }
            }
            div {
                input { placeholder: "订阅 URL", value: new_url, oninput: move |e| new_url.set(e.value()) }
                input { placeholder: "名称", value: new_name, oninput: move |e| new_name.set(e.value()) }
                button { onclick: add, "添加" }
            }
            table {
                thead {
                    tr { th { "ID" } th { "名称" } th { "URL" } th { "状态" } th { "操作" } }
                }
                tbody {
                    for s in sources.read().iter() {
                        tr {
                            td { "{s.id}" }
                            td { "{s.name}" }
                            td { "{s.url}" }
                            td {
                                span { class: format_args!("badge {}", if s.enabled { "on" } else { "off" }),
                                    if s.enabled { "启用" } else { "停用" }
                                }
                            }
                            td {
                                button { class: "secondary", onclick: move |_| toggle(s.id, s.enabled),
                                    if s.enabled { "停用" } else { "启用" }
                                }
                                button { class: "secondary", onclick: move |_| refresh(s.id), "刷新" }
                                button { class: "danger", onclick: move |_| del(s.id), "删除" }
                            }
                        }
                    }
                }
            }
        }
    }
}
```

- [ ] **Step 2: 更新 main.rs 加入 Tab 导航**

```rust
// main.rs 里 MainShell 结构：
#[component]
fn MainShell(token: Signal<Option<String>>) -> Element {
    let tab = use_signal(|| 0usize);
    rsx! {
        div { class: "container",
            h1 { "sub-merge 管理" }
            nav { style: "margin-bottom: 16px",
                button { class: "secondary", onclick: move |_| tab.set(0), "订阅源" }
                button { class: "secondary", onclick: move |_| tab.set(1), "预览" }
                button { class: "secondary", onclick: move |_| tab.set(2), "配置" }
                button { class: "danger", onclick: move |_| {
                    clear_token();
                    token.set(None);
                }, "退出登录" }
            }
            match *tab.read() {
                0 => rsx! { components::sources::Sources { token } },
                1 => rsx! { components::preview::Preview { token } },
                _ => rsx! { components::config::Config { token } },
            }
        }
    }
}
```

- [ ] **Step 3: 构建验证**

Run: `cd crates/server/web && dx build --web`
Expected: 成功（若 Preview/Config 未实现，先给空占位组件）

- [ ] **Step 4: Commit**

```bash
git add crates/server/web/
git commit -m "feat(web): sources management with CRUD UI"
```

---

### Task 4: 转换预览组件

**Files:**
- Create: `crates/server/web/src/components/preview.rs`

**Interfaces:**
- Consumes: `api::request`, `admin_token`
- Produces:
  - `#[derive(Deserialize)] struct PreviewNode { name: String, protocol: String, server: String, port: u16 }`
  - `pub fn Preview(token: Signal<Option<String>>) -> Element`

- [ ] **Step 1: 实现 preview.rs**

```rust
// crates/server/web/src/components/preview.rs
use crate::api::request;
use dioxus::prelude::*;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
struct PreviewNode {
    name: String,
    protocol: String,
    server: String,
    port: u16,
}

#[derive(Debug, Clone, Deserialize)]
struct PreviewResp {
    nodes: Vec<PreviewNode>,
    errors: Vec<String>,
    total: usize,
}

#[component]
pub fn Preview(token: Signal<Option<String>>) -> Element {
    let mut data = use_signal(|| None::<PreviewResp>);
    let mut loading = use_signal(|| false);
    let mut error = use_signal(String::new);

    let load = {
        let token = token.read().clone();
        let mut data = data.clone();
        let mut loading = loading.clone();
        let mut error = error.clone();
        spawn(async move {
            loading.set(true);
            match request("GET", "/api/admin/preview", None, token.as_deref()).await {
                Ok(body) => match serde_json::from_str::<PreviewResp>(&body) {
                    Ok(r) => data.set(Some(r)),
                    Err(e) => error.set(format!("解析失败: {}", e)),
                },
                Err(e) => error.set(e),
            }
            loading.set(false);
        });
    };
    let _ = load;

    rsx! {
        div { class: "card",
            h2 { "转换预览" }
            button { onclick: move |_| {
                // 重新加载
                let token = token.read().clone();
                let mut data = data.clone();
                let mut loading = loading.clone();
                let mut error = error.clone();
                spawn(async move {
                    loading.set(true);
                    match request("GET", "/api/admin/preview", None, token.as_deref()).await {
                        Ok(body) => match serde_json::from_str::<PreviewResp>(&body) {
                            Ok(r) => data.set(Some(r)),
                            Err(e) => error.set(format!("解析失败: {}", e)),
                        },
                        Err(e) => error.set(e),
                    }
                    loading.set(false);
                });
            }, "刷新预览" }
            if *loading.read() {
                p { "加载中..." }
            }
            if let Some(resp) = data.read().as_ref() {
                p { "共 {resp.total} 个节点" }
                table {
                    thead { tr { th { "名称" } th { "协议" } th { "服务器" } th { "端口" } } }
                    tbody {
                        for n in &resp.nodes {
                            tr {
                                td { "{n.name}" }
                                td { "{n.protocol}" }
                                td { "{n.server}" }
                                td { "{n.port}" }
                            }
                        }
                    }
                }
                if !resp.errors.is_empty() {
                    h4 { "源错误" }
                    ul {
                        for e in &resp.errors {
                            li { "{e}" }
                        }
                    }
                }
            }
        }
    }
}
```

- [ ] **Step 2: 构建验证**

Run: `cd crates/server/web && dx build --web`
Expected: 成功

- [ ] **Step 3: Commit**

```bash
git add crates/server/web/
git commit -m "feat(web): conversion preview component"
```

---

### Task 5: 配置页（订阅链接复制 + token 轮换）

**Files:**
- Create: `crates/server/web/src/components/config.rs`

**Interfaces:**
- Consumes: `api::request`, `admin_token`
- Produces:
  - `#[derive(Deserialize)] struct ConfigDto { subscribe_token: String, admin_token: String, subscribe_url: String }`
  - `pub fn Config(token: Signal<Option<String>>) -> Element`
  - 三个格式的订阅链接：`/api/subscribe?token=<sub>&format=clash|v2ray|singbox`
  - 复制到剪贴板（`web_sys::navigator::clipboard`）

- [ ] **Step 1: 实现 config.rs**

```rust
// crates/server/web/src/components/config.rs
use crate::api::request;
use dioxus::prelude::*;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
struct ConfigDto {
    subscribe_token: String,
    admin_token: String,
    subscribe_url: String,
}

#[component]
pub fn Config(token: Signal<Option<String>>) -> Element {
    let mut cfg = use_signal(|| None::<ConfigDto>);
    let mut error = use_signal(String::new);
    let mut copied = use_signal(String::new);

    let load = {
        let token = token.read().clone();
        let mut cfg = cfg.clone();
        spawn(async move {
            if let Ok(body) = request("GET", "/api/admin/config", None, token.as_deref()).await {
                if let Ok(c) = serde_json::from_str::<ConfigDto>(&body) {
                    cfg.set(Some(c));
                }
            }
        });
    };
    let _ = load;

    let rotate = move |which: &'static str| {
        let token = token.read().clone();
        let body = serde_json::json!({ "rotate": which }).to_string();
        let mut cfg = cfg.clone();
        spawn(async move {
            match request("PUT", "/api/admin/config", Some(&body), token.as_deref()).await {
                Ok(b) => {
                    if let Ok(c) = serde_json::from_str::<ConfigDto>(&b) {
                        cfg.set(Some(c));
                    }
                }
                Err(e) => error.set(e),
            }
        });
    };

    let copy = move |text: String| {
        let w = web_sys::window();
        if let Some(nav) = w.and_then(|w| w.navigator()) {
            if let Ok(_) = nav.clipboard() {
                let mut copied = copied.clone();
                spawn(async move {
                    let clip = nav.clipboard().unwrap();
                    let _ = clip.write_text(&text);
                    copied.set("已复制".into());
                });
            }
        }
    };

    rsx! {
        div { class: "card",
            h2 { "配置" }
            if let Some(c) = cfg.read().as_ref() {
                h3 { "订阅链接" }
                for (label, fmt) in [("Clash", "clash"), ("V2Ray", "v2ray"), ("Sing-box", "singbox")] {
                    let link = format!("{}/api/subscribe?token={}&format={}", web_sys::window().and_then(|w| w.location().href().ok()).unwrap_or_default(), c.subscribe_token, fmt);
                    div {
                        strong { "{label} " }
                        code { "{link}" }
                        button { class: "secondary", onclick: move |_| copy(link.clone()), "复制" }
                    }
                }
                if !copied.read().is_empty() {
                    p { style: "color: #1a7f37", "{copied}" }
                }
                hr {}
                h3 { "Token" }
                p { "订阅 token: " code { "{c.subscribe_token}" } }
                button { class: "secondary", onclick: move |_| rotate("subscribe"), "轮换订阅 token" }
                p { "管理 token: " code { "{c.admin_token}" } }
                button { class: "danger", onclick: move |_| rotate("admin"), "轮换管理 token" }
            }
            if !error.read().is_empty() {
                p { style: "color: #ff3b30", "{error}" }
            }
        }
    }
}
```

- [ ] **Step 2: 构建验证**

Run: `cd crates/server/web && dx build --web`
Expected: 成功

- [ ] **Step 3: 全链路联调（前端 + 后端）**

```bash
cd /root/github/sub-merge
# 1) 构建前端
(cd crates/server/web && dx build --web)
# 2) 启动后端（WEB_DIST 指向构建产物）
WEB_DIST=./crates/server/web/dist cargo run -p server &
sleep 3
# 3) 打开浏览器验证（手动）或 curl 检查静态资源
curl -s -o /dev/null -w "%{http_code}" http://localhost:8080/dist/index.js
# 应返回 200
```

- [ ] **Step 4: Commit**

```bash
git add crates/server/web/
git commit -m "feat(web): config page with subscribe links and token rotation"
```

---

### Task 6: Dockerfile + 完整构建脚本 + 联调收尾

**Files:**
- Create: `/root/github/sub-merge/Dockerfile`
- Create: `/root/github/sub-merge/docker-entrypoint.sh`（可选）
- Create: `/root/github/sub-merge/Makefile`（构建编排）

**Interfaces:**
- Consumes: Plan A/B/C 全部产物

- [ ] **Step 1: 创建 Dockerfile（多阶段）**

```dockerfile
# ---- 阶段 1: 构建前端 WASM ----
FROM rust:1.97 AS web-builder
RUN rustup target add wasm32-unknown-unknown
RUN cargo install dioxus-cli --locked || true  # 若失败用 dx 预装镜像
WORKDIR /app
COPY crates/server/web /app/crates/server/web
COPY Cargo.toml /app/Cargo.toml
WORKDIR /app/crates/server/web
RUN dx build --web || (cd /app/crates/server/web && ls)

# ---- 阶段 2: 构建 Rust 服务端 ----
FROM rust:1.97 AS server-builder
RUN apt-get update && apt-get install -y musl-tools || true
WORKDIR /app
COPY Cargo.toml /app/Cargo.toml
COPY crates /app/crates
COPY --from=web-builder /app/crates/server/web/dist /app/crates/server/web/dist
RUN cargo build --release -p server

# ---- 阶段 3: 运行时 ----
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=server-builder /app/target/release/server /app/server
COPY --from=web-builder /app/crates/server/web/dist /app/web/dist
ENV WEB_DIST=/app/web/dist DATABASE_PATH=/app/data/submerge.db PORT=8080
VOLUME ["/app/data"]
EXPOSE 8080
CMD ["/app/server"]
```

- [ ] **Step 2: 创建 Makefile**

```makefile
.PHONY: build-web build-server build run docker

build-web:
	cd crates/server/web && dx build --web

build-server:
	cargo build --release -p server

build: build-web build-server

run: build-web
	WEB_DIST=./crates/server/web/dist cargo run -p server

docker:
	docker build -t sub-merge .
```

- [ ] **Step 3: 本地完整构建验证**

```bash
make build-web
make build-server
```

- [ ] **Step 4: Docker 构建验证（若环境有 docker）**

```bash
docker build -t sub-merge .
docker run --rm -p 8080:8080 -e DATABASE_PATH=/app/data/submerge.db sub-merge
```

- [ ] **Step 5: 冒烟测试**

```bash
# 服务启动
# 登录管理界面（浏览器）
# 添加订阅源 → 预览 → 复制订阅链接
# curl 订阅链接验证输出
curl -s "http://localhost:8080/api/subscribe?token=<SUB>&format=clash" | head -20
```

- [ ] **Step 6: Commit**

```bash
git add Dockerfile Makefile
git commit -m "feat: docker multi-stage build and make orchestration"
```

---

## Plan C 完成标准

- [ ] `dx build --web` 成功产出 `dist/`
- [ ] 登录页：输入 token 验证后进入主界面
- [ ] 订阅源页：增删改、启停、刷新
- [ ] 预览页：节点列表 + 源错误展示
- [ ] 配置页：三个格式订阅链接 + 复制 + 两个 token 轮换
- [ ] 退出登录清除 localStorage
- [ ] Docker 多阶段构建成功，容器可运行
- [ ] 端到端：浏览器登录 → 加源 → 预览 → 订阅链接可 curl 出节点
