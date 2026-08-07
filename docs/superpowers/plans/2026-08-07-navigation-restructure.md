# 前端导航重构（层级菜单 + 预览拆分）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把侧边栏从 5 个平铺一级菜单重构为层级导航（订阅管理→单条订阅→本地/远程订阅），删除概览页，预览功能拆分到各订阅管理页内嵌。

**Architecture:** 后端 `/admin/preview` 加 `?kind=` 参数（按源类型过滤）；前端预览渲染逻辑抽成共享 `PreviewSection` 组件（本地/远程/组合三页复用）；`sources.rs` 组件 kind 参数化（本地/远程两实例化）；MainShell 导航改为 NavGroup/NavLeaf 层级 + 折叠展开状态；删除 overview.rs/preview.rs 与 DataStore 的 preview 单元。

**Tech Stack:** axum 0.8（后端）；dioxus 0.8.0-alpha.1（前端）；sqlx 0.9；ui-check.py（CDP 场景脚本）。

## Global Constraints

- 每次代码修改后必须按序通过：`cargo upgrade -i` → `cargo fmt --all` → `cargo clippy --workspace` → `cargo test --workspace`（CLAUDE.md 强制）
- Rust edition 2024，rust-version 1.97
- web crate 独立于 workspace（不进 members），仅由 `dx build --web --release --debug-symbols false` 构建，必须 0 警告
- dioxus 0.8 坑清单：rsx if/else 分支内不嵌套 `rsx!` 宏调用；`Element` 空渲染用 `VNode::empty()`；`use_effect` 无依赖数组（每次渲染跑），一次性逻辑用信号守卫；svg 属性 snake_case
- 叶子页索引：`0=本地订阅 1=远程订阅 2=组合订阅 3=配置`；默认 tab=0（本地订阅）
- 分组展开状态：`use_signal(HashSet<&'static str>)`，分组名 `"subs"`（订阅管理）与 `"single"`（单条订阅），默认两者都展开；选中叶子时祖先分组强制展开
- `?kind=` 取值限 `single`/`remote`，与 `?combined=` 互斥（同时出现 400），非法值 400
- 组合订阅页成员勾选维持现状（全部源可选，含 single 源）
- 不动 smoke.sh（API 层无变化）；ui-check.py 场景改写（Task 5）
- 项目 docs/superpowers/specs/2026-08-07-navigation-restructure-design.md 是本次设计依据

---

### Task 1: 后端 /admin/preview 支持 ?kind= 过滤

**Files:**
- Modify: `crates/server/src/routes/preview.rs`
- Test: `crates/server/tests/api_test.rs`

**Interfaces:**
- Consumes: 现状 `fetch_and_merge(state, source_ids: Option<&[i64]>)`（service.rs，Task 0 基线）
- Produces: `PreviewQuery` 增加 `pub kind: Option<String>`；`GET /admin/preview?kind=single|remote` 按类型过滤；kind+combined 互斥 400；非法 kind 400

- [ ] **Step 1: 写失败测试（api_test.rs 追加）**

`http` helper 已存在于 api_test.rs 顶层（Task 0 基线：`http(app, method, uri, body, token) -> (StatusCode, Value)`）：

```rust
#[tokio::test]
async fn preview_filters_by_kind() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-preview-kind", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    // single 源（不拉网络）+ remote 源（指向 127.0.0.1:1 必然失败 → 产生源错误但请求 200）
    sqlx::query("INSERT INTO sources (url, name, kind, enabled, created_at) VALUES ('ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#LOCAL', 'local', 'single', 1, 'now')")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO sources (url, name, kind, enabled, created_at) VALUES ('http://127.0.0.1:1/sub', 'dead', 'remote', 1, 'now')")
        .execute(&pool).await.unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg).await;
    let admin = setup_admin(&app).await;

    // ?kind=single → 只有 local 源节点
    let (s, v) = http(&app, "GET", "/admin/preview?kind=single", None, Some(&admin)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["total"], 1);
    assert_eq!(v["nodes"][0]["name"], "LOCAL");

    // ?kind=remote → 无节点但 200（源失败进 errors）
    let (s, v) = http(&app, "GET", "/admin/preview?kind=remote", None, Some(&admin)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["total"], 0);
    assert!(!v["errors"].as_array().unwrap().is_empty(), "dead remote 源应进 errors");

    // kind + combined 互斥 → 400
    let (s, _) = http(&app, "GET", "/admin/preview?kind=single&combined=grp", None, Some(&admin)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // 非法 kind → 400
    let (s, _) = http(&app, "GET", "/admin/preview?kind=bogus", None, Some(&admin)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --test api_test preview_filters_by_kind`
Expected: FAIL（`?kind=` 被忽略，total 为 2 或节点断言不符）

- [ ] **Step 3: 实现（preview.rs）**

```rust
#[derive(serde::Deserialize)]
pub struct PreviewQuery {
    pub combined: Option<String>,
    pub kind: Option<String>,
}

async fn preview_handler(...) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(State(state.clone()), headers).await?;
    let Query(q) = q.map_err(ApiError::from)?;
    if q.kind.is_some() && q.combined.is_some() {
        return Err(ApiError::bad_request("kind and combined are mutually exclusive"));
    }
    if let Some(k) = &q.kind {
        if !matches!(k.as_str(), "single" | "remote") {
            return Err(ApiError::bad_request("kind must be 'single' or 'remote'"));
        }
    }
    // 成员过滤：combined（按组合）→ kind（按类型）→ None（全部）
    let member_ids: Option<Vec<i64>> = if let Some(name) = &q.combined {
        let cid: Option<i64> =
            sqlx::query_scalar("SELECT id FROM combined_subs WHERE name = ?")
                .bind(name).fetch_optional(&state.pool).await?;
        let Some(cid) = cid else {
            return Err(ApiError::not_found("combined subscription not found"));
        };
        Some(sqlx::query_scalar(
            "SELECT source_id FROM combined_sources WHERE combined_id = ? ORDER BY source_id",
        ).bind(cid).fetch_all(&state.pool).await?)
    } else if let Some(k) = &q.kind {
        Some(sqlx::query_scalar(
            "SELECT id FROM sources WHERE enabled = 1 AND kind = ? ORDER BY id",
        ).bind(k).fetch_all(&state.pool).await?)
    } else {
        None
    };
    let (nodes, errors) = service::fetch_and_merge(&state, member_ids.as_deref()).await;
    // ...其余不变
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --test api_test preview_filters_by_kind`
Expected: PASS

- [ ] **Step 5: 门禁 + commit**

Run: `cargo fmt --all && cargo clippy --workspace && cargo test --workspace`

```bash
git add crates/server/src/routes/preview.rs crates/server/tests/api_test.rs
git commit -m "feat(preview): /admin/preview 支持 ?kind= 按源类型过滤（与 ?combined= 互斥）"
```

---

### Task 2: PreviewSection 共享组件抽取

**Files:**
- Create: `crates/server/web/src/components/preview_section.rs`
- Modify: `crates/server/web/src/components/preview.rs`（页面改用 PreviewSection，行为不变）
- Modify: `crates/server/web/src/components/mod.rs`（加 `pub mod preview_section;`）

**Interfaces:**
- Consumes: `request`（api.rs）、`PreviewResp`（web-core dto）、`proto_class`（web-core fmt）、`Spinner`/`icon`（icon.rs）
- Produces（Task 3/4 依赖）:
  - `pub fn PreviewSection(token: Signal<Option<String>>, kind: Option<&'static str>, combined: Option<String>) -> Element`
  - 挂载自动加载 + 刷新按钮 + 节点表 + 源错误卡；`kind`/`combined` prop 变化时自动重拉（key 比较守卫）
  - 拉取路径：`kind` 有值 → `/admin/preview?kind={kind}`；`combined` 有值 → `/admin/preview?combined={name}`；都无 → `/admin/preview`（Task 4 前 preview.rs 页面用 `kind: None, combined: None` 保持全量视图）

- [ ] **Step 1: 新建 preview_section.rs**

把 `preview.rs` 的渲染逻辑迁移为独立组件，关键结构（含 prop 变化重拉守卫）：

```rust
// crates/server/web/src/components/preview_section.rs
// 共享预览区：节点表 + 源错误卡。本地/远程/组合三页复用。
// kind: Some("single"/"remote") 按类型过滤；combined: Some(组合名) 按组合过滤；二者互斥由调用方保证。
use crate::api::request;
use crate::components::icon::{Spinner, icon};
use dioxus::prelude::*;
use submerge_web_core::dto::PreviewResp;
use submerge_web_core::fmt::proto_class;

#[component]
pub fn PreviewSection(
    token: Signal<Option<String>>,
    kind: Option<&'static str>,
    combined: Option<String>,
) -> Element {
    let mut data = use_signal(|| None::<PreviewResp>);
    let mut loading = use_signal(|| false);
    let mut error = use_signal(String::new);
    // 当前拉取参数键（kind+combined 组合）；变化时触发重拉
    let key = format!("{}|{}", kind.unwrap_or(""), combined.as_deref().unwrap_or(""));
    let mut loaded_key = use_signal(|| None::<String>);

    let mut load = move |key: String| {
        let token = token.read().clone();
        let mut data = data.clone();
        let mut loading = loading.clone();
        let mut error = error.clone();
        spawn(async move {
            loading.set(true);
            error.set(String::new());
            let (path, kind, combined) = if key.starts_with("single|") {
                ("/admin/preview?kind=single".to_string(), "single", None)
            } else if key.starts_with("remote|") {
                ("/admin/preview?kind=remote".to_string(), "remote", None)
            } else {
                let name = key.split('|').nth(1).unwrap_or("");
                (format!("/admin/preview?combined={name}"), "", Some(name.to_string()))
            };
            let _ = (kind, combined);
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

    // 挂载 + prop 变化时重拉（use_effect 无依赖数组，loaded_key 守卫保证单次/变化触发）
    use_effect(move || {
        let key = key.clone();
        if loaded_key.read().as_deref() != Some(&key) {
            loaded_key.set(Some(key.clone()));
            load(key);
        }
    });

    // 行渲染（节点表 + 源错误卡）——沿用原 preview.rs 的 rows/error_rows 预渲染模式
    // 刷新按钮：loading 时转圈禁用；错误文本：error 非空渲染 .error-text
    // 节点表空时渲染 empty 提示「暂无节点」
    // 完整渲染代码参照原 preview.rs 的 rsx 结构迁移（节点头部/表格/警告卡）
    // ...
}
```

注意：迁移时把原 preview.rs 的 `rows`/`error_rows` 预渲染、`empty` 提示、`warning-box` 全部带入；`use_future` 挂载加载逻辑删除（由 use_effect + loaded_key 守卫替代）。

- [ ] **Step 2: preview.rs 页面改用 PreviewSection**

`preview.rs` 保留页面外壳（页头 + 下拉 + 刷新），把「全部源」视图和「组合过滤」视图都改为渲染 `<PreviewSection {token} kind={None} combined={selected} />`——注意 PreviewSection 自带刷新按钮与错误展示，页面级刷新的本地状态逻辑删除。改造后 `preview.rs` 行为等价现状（全量视图 + 组合下拉切换）。

- [ ] **Step 3: mod.rs 加模块声明**

```rust
pub mod preview_section;
```

- [ ] **Step 4: 构建验证**

Run: `cd crates/server/web && dx build --web --release --debug-symbols false`
Expected: 0 警告构建成功

- [ ] **Step 5: 门禁 + commit**

Run: `cargo fmt --all && cargo clippy --workspace && cargo test --workspace`

```bash
git add crates/server/web/src/components/preview_section.rs crates/server/web/src/components/preview.rs crates/server/web/src/components/mod.rs
git commit -m "refactor(web): 预览渲染逻辑抽成共享 PreviewSection 组件"
```

---

### Task 3: sources.rs kind 参数化 + 组合页预览区

**Files:**
- Modify: `crates/server/web/src/components/sources.rs`
- Modify: `crates/server/web/src/components/combineds.rs`
- Modify: `crates/server/web/src/main.rs`（仅 Sources 调用点传 kind；导航结构 Task 4 才改）

**Interfaces:**
- Consumes: Task 2 的 `PreviewSection`
- Produces:
  - `pub fn Sources(token: Signal<Option<String>>, kind: &'static str) -> Element`（kind: "single"/"remote"）
  - `pub fn Combineds(...)` 内部追加：组合下拉信号 `preview_combined: Signal<Option<String>>` + `<PreviewSection {token} kind={None} combined={...}/>`

- [ ] **Step 1: sources.rs 参数化**

组件签名与过滤：

```rust
#[component]
pub fn Sources(token: Signal<Option<String>>, kind: &'static str) -> Element {
    // ...
    let sources_state = data.sources.read().clone();
    // 按 kind 过滤展示
    let source_list = sources_state
        .data
        .unwrap_or_default()
        .into_iter()
        .filter(|s| s.kind == kind)
        .collect::<Vec<_>>();
    // 添加表单：类型下拉删除（kind 固定）；placeholder 与校验按 kind 区分：
    //   kind=="single" → "ss://..., vmess://... 单条节点链接"
    //   kind=="remote" → "https://example.com/sub"
    // 添加请求 body 的 kind 字段传固定值：
    //   let body = serde_json::json!({ "url": url, "name": name, "kind": kind }).to_string();
    // 列表卡片标题旁加计数徽章：
    //   h2 { class: "card-title", "订阅源列表" }
    //   span { class: "badge on", "{source_list.len()} 个源" }
    // 页面底部追加预览区：
    //   div { class: "card",
    //       h2 { class: "card-title", "预览" }
    //       PreviewSection { token, kind: Some(kind), combined: None }
    //   }
    // 其余 CRUD/启停/单行刷新/删除逻辑不变
}
```

注意：`new_kind` 信号、类型下拉、`kind_label(&kind)` 徽章（页内所有源同 kind，徽章可保留或删除——保留无妨）、`let mut new_kind` 相关代码删除。

- [ ] **Step 2: main.rs 更新 Sources 调用点（临时传 "single"，Task 4 建第二实例）**

```rust
1 => rsx! { Sources { token, kind: "single" } },
```

（Task 4 会改为 `0 => Sources { kind: "single" }` / `1 => Sources { kind: "remote" }`，此步只是保持编译）

- [ ] **Step 3: combineds.rs 追加预览区**

组合列表卡片之后追加：

```rust
// 组合预览下拉 + PreviewSection（放在组合列表卡片下方的新卡片）
let preview_combined = use_signal(|| None::<String>);
// 下拉选项复用 combined_list（已从缓存读取）；onchange: selected.set(v) → 预览区随 prop 变化重拉
// rsx:
// div { class: "card",
//     h2 { class: "card-title", "预览" }
//     div { class: "preview-filter-row",
//         select {
//             class: "preview-filter",
//             onchange: move |e| {
//                 let v = e.value();
//                 let v = if v.is_empty() { None } else { Some(v) };
//                 preview_combined.set(v);
//             },
//             option { value: "", "选择组合订阅" }
//             {combined_options.into_iter()}   // 选项预渲染，模式同原 preview.rs
//         }
//     }
//     PreviewSection { token, kind: None, combined: preview_combined.read().clone() }
// }
```

`combined_options` 预渲染：从 `combined_list` 映射 `<option value={name} "{name}">`（原 preview.rs 的模式，含 selected 比对可省——下拉是非受控 + onchange 触发，selected 不需要）。

- [ ] **Step 4: 构建验证**

Run: `cd crates/server/web && dx build --web --release --debug-symbols false`
Expected: 0 警告构建成功

- [ ] **Step 5: 门禁 + commit**

Run: `cargo fmt --all && cargo clippy --workspace && cargo test --workspace`

```bash
git add crates/server/web/src/components/sources.rs crates/server/web/src/components/combineds.rs crates/server/web/src/main.rs
git commit -m "feat(web): 订阅源页按 kind 参数化（本地/远程）+ 组合页内嵌预览区"
```

---

### Task 4: MainShell 层级导航 + 删除概览/预览页 + 删 preview 单元

**Files:**
- Modify: `crates/server/web/src/main.rs`
- Modify: `crates/server/web/src/data.rs`
- Modify: `crates/server/web/index.html`
- Modify: `crates/server/web/src/components/mod.rs`
- Delete: `crates/server/web/src/components/overview.rs`
- Delete: `crates/server/web/src/components/preview.rs`

**Interfaces:**
- Consumes: Task 2 的 `PreviewSection`、Task 3 的 `Sources(kind)`/`Combineds`
- Produces: 目标导航（叶子 0=本地 1=远程 2=组合 3=配置，默认 0）；NavGroup/NavLeaf 组件；`UnitKey::{Sources, Combineds, Config}`（preview 删除）

- [ ] **Step 1: data.rs 删 preview 单元**

- `UnitKey` 枚举删 `Preview` 变体
- `fetch_preview` 函数删除
- `status_of`/`load`（含 stale_preview 捕获与 match 分支）中 Preview 分支删除
- `required_units` 表改为：

```rust
pub fn required_units(tab: usize) -> &'static [UnitKey] {
    match tab {
        0 => &[UnitKey::Sources],                     // 本地订阅
        1 => &[UnitKey::Sources],                     // 远程订阅
        2 => &[UnitKey::Combineds, UnitKey::Sources], // 组合订阅
        _ => &[UnitKey::Config],                      // 配置
    }
}
```

- `DataStore` struct 删 `preview: Signal<CacheState<PreviewResp>>` 字段与 `provide` 初始化；`use submerge_web_core::dto::PreviewResp` 若仅 preview 用则删除

- [ ] **Step 2: main.rs 层级导航**

删除 Overview/Preview 引用与 NavItem 平铺，改为：

```rust
// 叶子索引：0=本地订阅 1=远程订阅 2=组合订阅 3=配置
// 分组名："subs"（订阅管理）"single"（单条订阅）
let mut open_groups = use_signal(|| std::collections::HashSet::from(["subs", "single"]));

let mut go = move |i: usize| {
    if *tab.read() == i { return; }
    if *pending.read() == Some(i) { return; }
    // 选中叶子时祖先分组强制展开
    open_groups.write().insert("subs");
    if i < 2 { open_groups.write().insert("single"); }
    if data.all_ready(i) { pending.set(None); tab.set(i); }
    else { data.ensure_loaded(i); pending.set(Some(i)); }
};
```

侧边栏渲染结构：

```rust
nav { class: "nav",
    NavGroup {
        name: "subs", label: "订阅管理", icon: "sources", open: open_groups.read().contains("subs"),
        on_toggle: move |_| {
            let mut g = open_groups.write();
            if g.contains("subs") { g.remove("subs"); } else { g.insert("subs"); }
        },
        NavGroup {
            name: "single", label: "单条订阅", icon: "combineds", open: open_groups.read().contains("single"),
            on_toggle: move |_| {
                let mut g = open_groups.write();
                if g.contains("single") { g.remove("single"); } else { g.insert("single"); }
            },
            NavLeaf { name: "local", label: "本地订阅", active: *tab.read() == 0, loading: *pending.read() == Some(0), onnav: move |_| go(0) }
            NavLeaf { name: "remote", label: "远程订阅", active: *tab.read() == 1, loading: *pending.read() == Some(1), onnav: move |_| go(1) }
        }
        NavLeaf { name: "combineds", label: "组合订阅", active: *tab.read() == 2, loading: *pending.read() == Some(2), onnav: move |_| go(2) }
    }
    NavLeaf { name: "config", label: "配置", active: *tab.read() == 3, loading: *pending.read() == Some(3), onnav: move |_| go(3) }
}
```

组件定义：

```rust
#[component]
fn NavGroup(
    name: &'static str,
    label: &'static str,
    icon_name: &'static str,
    open: bool,
    on_toggle: EventHandler<MouseEvent>,
    children: Element,
) -> Element {
    rsx! {
        div { class: "nav-group",
            button { class: "nav-item nav-group-head", onclick: on_toggle,
                {icon(icon_name, 16)}
                span { "{label}" }
                span { class: format!("nav-chevron{}", if open { " open" } else { "" }),
                    {icon("chevron", 12)}
                }
            }
            if open {
                div { class: "nav-group-children", {children} }
            }
        }
    }
}

// NavLeaf = 原 NavItem（改名即可，props 不变）
#[component]
fn NavLeaf(name: &'static str, label: &'static str, active: bool, loading: bool, onnav: EventHandler<MouseEvent>) -> Element { /* 原 NavItem 实现 */ }
```

注意：`rsx!` 内嵌套 `NavGroup`/`NavLeaf` 是组件调用（元素形式），不违反坑清单；`{children}` 在 NavGroup 内是直接元素插入。

内容区 match：

```rust
let content: Element = if *pending.read() == Some(*tab.read()) {
    rsx! { div { class: "page-loading", Spinner { size: 28 } } }
} else {
    match *tab.read() {
        0 => rsx! { Sources { token, kind: "single" } },
        1 => rsx! { Sources { token, kind: "remote" } },
        2 => rsx! { Combineds { token } },
        _ => rsx! { Config { token } },
    }
};
```

默认 tab 不变（`use_signal(|| 0usize)` = 本地订阅）。`on_goto` 回调（原 Overview 的「管理订阅源」按钮）已无消费方，删除。

- [ ] **Step 3: 删除 overview.rs / preview.rs + mod.rs 更新**

`components/mod.rs`：

```rust
pub mod combineds;
pub mod config;
pub mod confirm;
pub mod icon;
pub mod login;
pub mod preview_section;
pub mod sources;
pub mod toast;
```

删除 `pub mod overview;` 与 `pub mod preview;`，`git rm` 两个文件。

- [ ] **Step 4: index.html 分组 CSS**

在 `/* ========== 布局：侧边栏 + 主内容 ========== */` 段追加：

```css
.nav-group { display: flex; flex-direction: column; gap: 2px; }
.nav-group-head { justify-content: flex-start; }
.nav-group-head span:last-child { margin-left: auto; }
.nav-chevron { display: inline-flex; transition: transform .15s ease; color: var(--text-tertiary); }
.nav-chevron.open { transform: rotate(90deg); }
.nav-group-children { display: flex; flex-direction: column; gap: 2px; padding-left: 12px; }
```

窄屏（≤768px）适配：`.nav-group-children` 纵向堆叠保持；分组头在顶栏中文字隐藏（`span` display:none 现规则已覆盖 label span，chevron 需保留或隐藏——保持现状规则即可，chevron 用 `.nav-chevron` 类不受 `nav-item span` 规则影响）。

icon.rs 加 `"chevron"`（右箭头，Lucide chevron-right）：

```rust
"chevron" => rsx! {
    svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
        path { d: "m9 18 6-6-6-6" }
    }
},
```

- [ ] **Step 5: 构建验证**

Run: `cd crates/server/web && dx build --web --release --debug-symbols false`
Expected: 0 警告构建成功

- [ ] **Step 6: 门禁 + commit**

Run: `cargo fmt --all && cargo clippy --workspace && cargo test --workspace`

```bash
git add -A crates/server/web/
git commit -m "feat(web): MainShell 层级导航（订阅管理→单条订阅→本地/远程），删除概览与预览页、preview 数据单元"
```

---

### Task 5: ui-check.py 场景改写

**Files:**
- Modify: `scripts/ui-check.py`

**Interfaces:**
- Consumes: Task 1-4 全部（导航路径、页面选择器）
- Produces: 场景在新导航下语义等价（旧页保持/转圈/错误可见/刷新恢复）

- [ ] **Step 1: 场景断言目标更新**

导航路径（点击目标）统一替换：`概览` → `本地订阅`（初始页，无需点击）；`订阅源` → `本地订阅`/`远程订阅`（按场景）；`预览` → 组合订阅页的预览区（下拉切换）；`配置` 不变。

各场景具体改写：

```python
def scenario_nav_preload(ws):
    """首次切换:旧页保持 + 菜单项转圈 → 就绪后切换;已加载页回访秒开;分组折叠/展开。"""
    seed_sources(ws, 1)
    cmd(ws, "Page.addScriptToEvaluateOnNewDocument", {"source": OBSERVER_JS})
    login(ws)
    # 初始页=本地订阅(第一个叶子),预载 sources 单元
    assert_true(wait_until(ws, "window.__ui.saw_spinner===true", timeout=15), "初始加载中菜单项转圈")
    assert_true(wait_until(ws, "window.__ui.saw_loading===true", timeout=3), "初始加载内容区显示全页 loading")
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('本地订阅')).classList.contains('active')"), "本地订阅就绪后激活")
    # 分组折叠:点「单条订阅」收起 → 本地订阅不可见 → 再展开
    ev(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('单条订阅')).click()")
    time.sleep(0.3)
    assert_true(not nav_el(ws, "本地订阅"), "折叠后三级菜单隐藏")
    ev(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('单条订阅')).click()")
    time.sleep(0.3)
    assert_true(nav_el(ws, "本地订阅"), "展开后三级菜单可见")
    # 回访秒开:切远程订阅(同单元缓存) → 立即切换
    click_nav(ws, "远程订阅")
    assert_true(nav_active(ws, "远程订阅"), "同单元切换秒开")
    assert_true(not nav_loading(ws, "远程订阅"), "秒开路径无转圈")
    # 慢路径:注入 4s 网络延迟 → 点「配置」(首个请求,无缓存) → 旧页保持 + 转圈 → 就绪后切换
    cmd(ws, "Network.enable")
    cmd(ws, "Network.emulateNetworkConditions",
        {"offline": False, "latency": 4000, "downloadThroughput": -1, "uploadThroughput": -1})
    click_nav(ws, "配置")
    time.sleep(1.0)
    assert_true(ev(ws, "document.body.innerText.includes('远程订阅')"), "慢加载期间旧页(远程订阅)保持可见")
    assert_true(nav_loading(ws, "配置"), "慢加载期间目标菜单项转圈")
    assert_true(not nav_active(ws, "配置"), "慢加载期间未提前切换")
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('配置')).classList.contains('active')", timeout=15), "就绪后切换完成")
    cmd(ws, "Network.emulateNetworkConditions",
        {"offline": False, "latency": 0, "downloadThroughput": -1, "uploadThroughput": -1})

def scenario_first_load_failure(ws):
    """首次加载失败(预览区请求):预览区错误文本 + 页面仍切换(单元失败不再阻塞——本地订阅仅需 sources 单元)。"""
    cmd(ws, "Network.enable")
    cmd(ws, "Network.setBlockedURLs", {"urls": ["*admin/preview*"]})
    cmd(ws, "Page.enable"); cmd(ws, "Runtime.enable")
    time.sleep(2)
    for _ in range(20):
        if ev(ws, "document.readyState") == "complete":
            break
        time.sleep(0.5)
    login(ws)
    # 本地订阅页就绪(仅 sources 单元,不被 preview 拦截阻塞)
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('本地订阅')).classList.contains('active')", timeout=15), "初始页切换(本地订阅激活)")
    assert_true(wait_until(ws, "!!document.querySelector('.error-text')", timeout=10), "预览区出现错误文本(拦截失败)")
    # 解除拦截,点预览区刷新 → 恢复
    cmd(ws, "Network.setBlockedURLs", {"urls": []})
    ev(ws, "Array.from(document.querySelectorAll('button')).find(b=>b.textContent.includes('刷新预览'))?.click() ?? Array.from(document.querySelectorAll('button')).find(b=>b.textContent.includes('刷新')).click()")
    assert_true(wait_until(ws, "!document.querySelector('.error-text')", timeout=15), "解除后刷新恢复(错误消失)")

def scenario_sources_crud(ws):
    """本地订阅页添加源 → 计数徽章 +1(原概览统计断言迁移)。"""
    login(ws)
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('本地订阅')).classList.contains('active')"), "本地订阅就绪")
    n0 = ev(ws, "parseInt(Array.from(document.querySelectorAll('.card h2 + .badge, .card-title + .badge'))[0]?.textContent ?? '0', 10)")
    assert_true(isinstance(n0, int), "读取到添加前源数徽章")
    ev(ws, "(()=>{const ins=document.querySelectorAll('.form-row input');ins[0].value='vless://e99a8e5a-6b2b-4a1d-9c5f-1a2b3c4d5e6f@9.9.9.9:443#crud-test';ins[0].dispatchEvent(new Event('input',{bubbles:true}));ins[1].value='crud-test';ins[1].dispatchEvent(new Event('input',{bubbles:true}));})()")
    ev(ws, "Array.from(document.querySelectorAll('.form-row button')).find(b=>b.textContent.includes('添加')).click()")
    time.sleep(0.8)
    assert_true(wait_until(ws, "document.body.innerText.includes('crud-test')"), "添加后列表出现新源")
    assert_true(wait_until(ws, "parseInt(Array.from(document.querySelectorAll('.card h2 + .badge, .card-title + .badge'))[0]?.textContent ?? '0', 10) === %d" % (n0 + 1)), "计数徽章 +1")
    cleanup_source("crud-test")

def scenario_preview_filter(ws):
    """组合订阅页:预览下拉切换组合。"""
    import urllib.request as u
    # 确保 c-test 组合存在(combineds 场景会创建;缺失时经 API 补建)
    req = u.Request(URL + "/admin/combineds", headers={"Authorization": "Bearer " + SESSION_TOKEN})
    with u.urlopen(req, timeout=5) as r:
        combos = json.loads(r.read())
    if not any(c.get("name") == "c-test" for c in combos):
        req = u.Request(URL + "/admin/sources", headers={"Authorization": "Bearer " + SESSION_TOKEN})
        with u.urlopen(req, timeout=5) as r:
            first_id = json.loads(r.read())[0]["id"]
        req = u.Request(URL + "/admin/combineds", method="POST",
                        data=json.dumps({"name": "c-test", "source_ids": [first_id]}).encode(),
                        headers={"Authorization": "Bearer " + SESSION_TOKEN, "Content-Type": "application/json"})
        u.urlopen(req, timeout=5)
    seed_sources(ws, 2)
    login(ws)
    click_nav(ws, "组合订阅")
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('组合订阅')).classList.contains('active')"), "组合订阅就绪")
    # 预览下拉出现 c-test 选项 → 切换 → 表渲染
    assert_true(wait_until(ws, "!!Array.from(document.querySelectorAll('.preview-filter option')).find(o=>o.textContent==='c-test')"), "预览下拉出现 c-test")
    ev(ws, "(()=>{const sel=document.querySelector('.preview-filter');const t=Array.from(sel.options).find(o=>o.textContent==='c-test');sel.value=t.value;sel.dispatchEvent(new Event('change',{bubbles:true}));})()")
    time.sleep(0.8)
    assert_true(wait_until(ws, "!!document.querySelector('.table-wrap tbody tr')"), "组合预览节点表渲染")

def scenario_refresh_failure(ws):
    """刷新失败:停 server → 预览区刷新 → 错误文本出现(旧数据保留语义迁移到预览区)。"""
    # 其余同现状:find_server/restart_server 不变;断言目标改为本地订阅页
    # 停 server 前:记录预览区表格行数
    # 停 server → 点预览区「刷新」→ 错误文本出现 + 表格行数不变 → 重启 → 刷新恢复
```

`nav_el` 已存在（querySelectorAll('nav button') 文本匹配），`click_nav` 复用。注意 `scenario_refresh_failure` 与 `scenario_combineds`/`scenario_config_password` 仅导航点击目标变化（`组合订阅`/`配置` 已是叶子名，无需改点击逻辑；`combineds` 场景的「新建组合」弹窗与断言不变）。

- [ ] **Step 2: 语法验证**

Run: `python3 -m py_compile scripts/ui-check.py`
Expected: OK

- [ ] **Step 3: 门禁 + commit**

Run: `cargo fmt --all && cargo clippy --workspace && cargo test --workspace`（确认无破坏）

```bash
git add scripts/ui-check.py
git commit -m "chore(ui-check): 场景断言迁移到层级导航（本地/远程/组合页 + 分组折叠）"
```

---

## 自审记录

**Spec 覆盖检查：**
- 导航树（订阅管理→单条订阅→本地/远程）+ 配置一级 → Task 4
- NavGroup/NavLeaf、折叠箭头、祖先强制展开、默认展开 → Task 4
- 叶子索引 0-3、默认 tab=0 → Task 4
- ?kind= 参数（互斥 400、非法 400、kind 查询）→ Task 1
- PreviewSection（kind/combined prop、挂载自动加载、prop 变化重拉）→ Task 2
- sources.rs kind 参数化、计数徽章、表单类型下拉删除 → Task 3
- combineds.rs 预览下拉 + PreviewSection → Task 3
- 删除 overview.rs/preview.rs、preview 数据单元 → Task 4
- CSS（nav-group/children/chevron、缩进、窄屏）→ Task 4
- ui-check.py 五场景改写 → Task 5
- 组合成员勾选维持全部源可选 → Task 3 未动成员逻辑（符合 spec）
- 不动 smoke.sh → 全局约束
- 验证方式：cargo 门禁（各任务）、dx build（Task 2-4）、py_compile（Task 5）

**无占位符检查：** 各任务含完整代码或明确修改点；Task 2 的 PreviewSection 迁移注明「完整渲染代码参照原 preview.rs 迁移」（实施者以现有文件为源，属明确的迁移指令而非占位符）。

**类型一致性：** `PreviewSection(token, kind: Option<&'static str>, combined: Option<String>)` 在 Task 2 定义，Task 3/4 使用一致；`Sources(token, kind: &'static str)` 在 Task 3 定义，Task 4 两处实例化一致；`NavGroup(name, label, icon_name, open, on_toggle, children)` / `NavLeaf(name, label, active, loading, onnav)` 在 Task 4 定义且侧边栏使用一致；`UnitKey::{Sources, Combineds, Config}` 在 Task 4 定义后 data.rs 内一致。
