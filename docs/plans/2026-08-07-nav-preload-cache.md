# 导航预载缓存实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 菜单切换从「先切页面再载内容」改为「先载入内容再显示页面」——旧页保持可见、菜单项转圈、就绪后瞬间切换,已访问页缓存秒开。

**Architecture:** 新增 `crates/server/web/src/data.rs` 集中数据层:四个 API 拉取函数 + `DataStore`(四个单元缓存信号 sources/combineds/preview/config,按单元而非按页共享)。MainShell 持有 DataStore 并编排切换(目标 tab 所需单元全 Ready 秒开;否则预载缺失单元、旧页保持 + 菜单项转圈、完成后再切)。各页删除挂载 use_future,改为从缓存读;CRUD/刷新成功后 `refresh` 对应单元回写缓存。

**Tech Stack:** dioxus 0.8.0-alpha.1(WASM,web-sys/gloo-net)、dx CLI 构建、web-core DTO。

**Spec:** `docs/specs/2026-08-07-nav-preload-cache-design.md`

## Global Constraints

- web crate 只能由 `dx build` 构建,必须带 `--debug-symbols false`(binaryen DWARF 崩溃,见 CLAUDE.md 坑清单)
- dioxus 0.8.0-alpha.1 精确锁定;edition 2024 字面声明,不继承 workspace
- rsx 规则:if/else 分支内**不能嵌套 `rsx!`**(写元素形式);match 分支嵌套 `rsx!` 可用;组件空渲染用 `VNode::empty()`
- `Signal::set/write` 需 `mut` 绑定(`let mut s = store.sources; s.set(...)`)
- `use_effect` 无依赖数组(每次渲染跑),一次性逻辑用信号守卫
- web crate 不在 workspace:`cargo fmt/clippy/test` 不覆盖它,但每任务完成后照跑门禁(CLAUDE.md 强制序列)
- 验证 = `dx build`(0 警告)+ `make smoke` + CDP 浏览器检查(`scripts/ui-check.py`)
- 前端无测试 harness:不写单元测试,行为验证全靠 CDP 断言

---

### Task 1: data.rs 数据层模块(类型 + fetch 函数集中)

**Files:**
- Create: `crates/server/web/src/data.rs`
- Modify: `crates/server/web/src/main.rs`(加 `mod data;`)
- Modify: `crates/server/web/src/components/sources.rs:3-14`(删 fetch_sources 定义,换 import)
- Modify: `crates/server/web/src/components/combineds.rs:6,14-17`(删 fetch_combineds 定义,换 import)
- Modify: `crates/server/web/src/components/overview.rs:6,11-14`(删 fetch_preview,换 import)
- Modify: `crates/server/web/src/components/preview.rs:5`(换 import)
- Modify: `crates/server/web/src/components/config.rs:22-35`(use_future 内联拉取换 fetch_config)

**Interfaces:**
- Produces(后续任务依赖):
  - `pub async fn fetch_sources(token: Option<&str>) -> Result<Vec<SourceDto>, String>`
  - `pub async fn fetch_combineds(token: Option<&str>) -> Result<Vec<CombinedDto>, String>`
  - `pub async fn fetch_preview_summary(token: Option<&str>) -> Result<PreviewSummary, String>`(Task 3 删除)
  - `pub async fn fetch_config(token: Option<&str>) -> Result<ConfigDto, String>`

- [ ] **Step 1: 创建 data.rs**

```rust
// crates/server/web/src/data.rs
// 集中式数据层:四个 API 拉取函数 + 页面间共享的单元缓存(DataStore)。
// 页面不再各自 use_future 拉取,改为从 DataStore 读缓存;MainShell 编排预载。
use crate::api::request;
use submerge_web_core::dto::{CombinedDto, ConfigDto, PreviewSummary, SourceDto};

pub async fn fetch_sources(token: Option<&str>) -> Result<Vec<SourceDto>, String> {
    let body = request("GET", "/admin/sources", None, token).await?;
    serde_json::from_str(&body).map_err(|e| format!("解析失败: {}", e))
}

pub async fn fetch_combineds(token: Option<&str>) -> Result<Vec<CombinedDto>, String> {
    let body = request("GET", "/admin/combineds", None, token).await?;
    serde_json::from_str(&body).map_err(|e| format!("解析失败: {}", e))
}

pub async fn fetch_preview_summary(token: Option<&str>) -> Result<PreviewSummary, String> {
    let body = request("GET", "/admin/preview", None, token).await?;
    serde_json::from_str(&body).map_err(|e| format!("解析失败: {}", e))
}

pub async fn fetch_config(token: Option<&str>) -> Result<ConfigDto, String> {
    let body = request("GET", "/admin/config", None, token).await?;
    serde_json::from_str(&body).map_err(|e| format!("解析失败: {}", e))
}
```

- [ ] **Step 2: main.rs 注册模块**

`crates/server/web/src/main.rs` 第 2 行 `mod api;` 后加一行 `mod data;`。

- [ ] **Step 3: 各组件换 import 并删本地定义**

sources.rs:
- 删除第 11-14 行 fetch_sources 定义
- 第 3 行 `use crate::api::request;` 保留(CRUD 仍用)
- 加一行 `use crate::data::fetch_sources;`

combineds.rs:
- 删除第 14-17 行 fetch_combineds 定义
- 第 6 行 `use crate::components::sources::fetch_sources;` 改为 `use crate::data::{fetch_combineds, fetch_sources};`

overview.rs:
- 删除第 11-14 行 fetch_preview 定义
- 第 6 行 `use crate::components::sources::fetch_sources;` 改为 `use crate::data::{fetch_preview_summary, fetch_sources};`

preview.rs:
- 第 5 行 `use crate::components::combineds::fetch_combineds;` 改为 `use crate::data::fetch_combineds;`

config.rs use_future(第 22-35 行)改为调用 fetch_config:
- 顶部加 `use crate::data::fetch_config;`
- use_future 内第 27 行 `match request("GET", "/admin/config", None, token.as_deref()).await {` 整段换成:

```rust
        async move {
            match fetch_config(token.as_deref()).await {
                Ok(c) => cfg.set(Some(c)),
                Err(e) => error.set(e),
            }
        }
```

- [ ] **Step 4: 构建验证**

Run: `cd crates/server/web && dx build --web --release --debug-symbols false`
Expected: exit 0,0 个 warning/error(本次无行为变化,纯搬移)

- [ ] **Step 5: 提交**

```bash
git add crates/server/web/src/data.rs crates/server/web/src/main.rs \
  crates/server/web/src/components/sources.rs crates/server/web/src/components/combineds.rs \
  crates/server/web/src/components/overview.rs crates/server/web/src/components/preview.rs \
  crates/server/web/src/components/config.rs
git commit -m "refactor(web): API 拉取函数集中到 data.rs 数据层"
```

---

### Task 2: DataStore 单元缓存 + MainShell 预载编排 + NavItem 转圈

**Files:**
- Modify: `crates/server/web/src/data.rs`(加 UnitKey/CacheStatus/CacheState/DataStore/fetch_preview)
- Modify: `crates/server/web/src/main.rs`(MainShell 编排 + NavItem loading prop + 内容区占位)
- Modify: `crates/server/web/index.html`(加 `.page-loading` 样式)
- Create: `scripts/ui-check.py`(CDP 行为检查脚手架 + nav_preload 场景)

**Interfaces:**
- Consumes: Task 1 的 `data::fetch_sources/fetch_combineds/fetch_config`
- Produces(后续任务依赖):
  - `enum UnitKey { Sources, Combineds, Preview, Config }`
  - `enum CacheStatus { Idle, Loading, Ready, Error }`(PartialEq)
  - `struct CacheState<T> { status: CacheStatus, data: Option<T>, error: String }`(Clone + Default)
  - `struct DataStore { sources: Signal<CacheState<Vec<SourceDto>>>, combineds: Signal<CacheState<Vec<CombinedDto>>>, preview: Signal<CacheState<PreviewResp>>, config: Signal<CacheState<ConfigDto>>, token: Signal<Option<String>> }`(Clone + Copy)
  - `DataStore::provide(token) -> DataStore`(use_context_provider)
  - `DataStore::required_units(tab: usize) -> &'static [UnitKey]`
  - `DataStore::status_of(&self, key) -> CacheStatus`
  - `DataStore::all_ready(&self, tab) -> bool` / `all_finished(&self, tab) -> bool` / `any_idle(&self, tab) -> bool`
  - `DataStore::ensure_loaded(&self, tab)` / `DataStore::refresh(&self, key)`
  - `pub async fn fetch_preview(token) -> Result<PreviewResp, String>`

- [ ] **Step 1: data.rs 追加缓存类型与 DataStore**

在 data.rs 追加(import 加 `use std::collections::HashSet;` 与 `use dioxus::prelude::*;` 与 `use submerge_web_core::dto::PreviewResp;`):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnitKey { Sources, Combineds, Preview, Config }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheStatus { Idle, Loading, Ready, Error }

#[derive(Debug, Clone)]
pub struct CacheState<T> {
    pub status: CacheStatus,
    pub data: Option<T>,
    pub error: String,
}

impl<T> Default for CacheState<T> {
    fn default() -> Self {
        Self { status: CacheStatus::Idle, data: None, error: String::new() }
    }
}

pub async fn fetch_preview(token: Option<&str>) -> Result<PreviewResp, String> {
    let body = request("GET", "/admin/preview", None, token).await?;
    serde_json::from_str(&body).map_err(|e| format!("解析失败: {}", e))
}

/// 页面共享的单元缓存 + 拉取编排。由 MainShell 经 use_context_provider 提供。
#[derive(Clone, Copy)]
pub struct DataStore {
    pub sources: Signal<CacheState<Vec<SourceDto>>>,
    pub combineds: Signal<CacheState<Vec<CombinedDto>>>,
    pub preview: Signal<CacheState<PreviewResp>>,
    pub config: Signal<CacheState<ConfigDto>>,
    pub token: Signal<Option<String>>,
    in_flight: Signal<HashSet<UnitKey>>,
}

impl DataStore {
    pub fn provide(token: Signal<Option<String>>) -> DataStore {
        use_context_provider(move || DataStore {
            sources: Signal::new(CacheState::default()),
            combineds: Signal::new(CacheState::default()),
            preview: Signal::new(CacheState::default()),
            config: Signal::new(CacheState::default()),
            token,
            in_flight: Signal::new(HashSet::new()),
        })
    }

    pub fn required_units(tab: usize) -> &'static [UnitKey] {
        match tab {
            0 => &[UnitKey::Sources, UnitKey::Preview], // 概览
            1 => &[UnitKey::Sources],                    // 订阅源
            2 => &[UnitKey::Combineds, UnitKey::Sources], // 组合订阅
            3 => &[UnitKey::Preview, UnitKey::Combineds], // 预览
            _ => &[UnitKey::Config],                     // 配置
        }
    }

    pub fn status_of(&self, key: UnitKey) -> CacheStatus {
        match key {
            UnitKey::Sources => self.sources.read().status,
            UnitKey::Combineds => self.combineds.read().status,
            UnitKey::Preview => self.preview.read().status,
            UnitKey::Config => self.config.read().status,
        }
    }

    /// 目标 tab 所需单元全部 Ready(秒开判断)
    pub fn all_ready(&self, tab: usize) -> bool {
        Self::required_units(tab).iter().all(|k| self.status_of(*k) == CacheStatus::Ready)
    }

    /// 目标 tab 所需单元已全部完成(Ready/Error 都算,无 Loading)
    pub fn all_finished(&self, tab: usize) -> bool {
        Self::required_units(tab).iter().all(|k| self.status_of(*k) != CacheStatus::Loading)
    }

    /// 目标 tab 是否还有从未加载的单元(初始自动加载判断;Error 不算,避免死循环)
    pub fn any_idle(&self, tab: usize) -> bool {
        Self::required_units(tab).iter().any(|k| self.status_of(*k) == CacheStatus::Idle)
    }

    /// 启动目标 tab 缺失(Idle/Error)单元的加载;Loading/Ready 跳过
    pub fn ensure_loaded(&self, tab: usize) {
        for key in Self::required_units(tab) {
            let st = self.status_of(*key);
            if st != CacheStatus::Ready && st != CacheStatus::Loading {
                self.load(*key);
            }
        }
    }

    /// 强制重拉单元(刷新按钮 / CRUD 回写)。加载期间保留旧 data,页面旧数据继续可读。
    pub fn refresh(&self, key: UnitKey) {
        self.load(key);
    }

    fn load(&self, key: UnitKey) {
        if self.in_flight.read().contains(&key) {
            return; // 单飞:同单元并发只拉一次
        }
        let store = *self;
        let mut in_flight = store.in_flight;
        // 立即置 Loading(保留旧 data),UI 即刻感知
        match key {
            UnitKey::Sources => {
                let cur = store.sources.read().clone();
                let mut s = store.sources;
                s.set(CacheState { status: CacheStatus::Loading, data: cur.data, error: String::new() });
            }
            UnitKey::Combineds => {
                let cur = store.combineds.read().clone();
                let mut s = store.combineds;
                s.set(CacheState { status: CacheStatus::Loading, data: cur.data, error: String::new() });
            }
            UnitKey::Preview => {
                let cur = store.preview.read().clone();
                let mut s = store.preview;
                s.set(CacheState { status: CacheStatus::Loading, data: cur.data, error: String::new() });
            }
            UnitKey::Config => {
                let cur = store.config.read().clone();
                let mut s = store.config;
                s.set(CacheState { status: CacheStatus::Loading, data: cur.data, error: String::new() });
            }
        }
        in_flight.write().insert(key);
        spawn(async move {
            let token = store.token.read().clone();
            let mut in_flight = store.in_flight;
            match key {
                UnitKey::Sources => {
                    let next = match fetch_sources(token.as_deref()).await {
                        Ok(d) => CacheState { status: CacheStatus::Ready, data: Some(d), error: String::new() },
                        Err(e) => CacheState { status: CacheStatus::Error, data: None, error: e },
                    };
                    let mut s = store.sources;
                    s.set(next);
                }
                UnitKey::Combineds => {
                    let next = match fetch_combineds(token.as_deref()).await {
                        Ok(d) => CacheState { status: CacheStatus::Ready, data: Some(d), error: String::new() },
                        Err(e) => CacheState { status: CacheStatus::Error, data: None, error: e },
                    };
                    let mut s = store.combineds;
                    s.set(next);
                }
                UnitKey::Preview => {
                    let next = match fetch_preview(token.as_deref()).await {
                        Ok(d) => CacheState { status: CacheStatus::Ready, data: Some(d), error: String::new() },
                        Err(e) => CacheState { status: CacheStatus::Error, data: None, error: e },
                    };
                    let mut s = store.preview;
                    s.set(next);
                }
                UnitKey::Config => {
                    let next = match fetch_config(token.as_deref()).await {
                        Ok(d) => CacheState { status: CacheStatus::Ready, data: Some(d), error: String::new() },
                        Err(e) => CacheState { status: CacheStatus::Error, data: None, error: e },
                    };
                    let mut s = store.config;
                    s.set(next);
                }
            }
            in_flight.write().remove(&key);
        });
    }
}
```

注意:Signal 的 `set/write` 需要 `mut` 绑定,故所有写操作前先拷贝出 `let mut xxx = store.xxx;`(与代码库既有模式一致)。

- [ ] **Step 2: MainShell 预载编排**

`main.rs` 的 `MainShell`(第 76-118 行)整体替换为:

```rust
// 主壳:侧边栏导航(窄屏自动收成顶栏,见 CSS @media 768px)。
// 切换策略:目标 tab 所需数据单元全部就绪才切换(旧页保持 + 菜单项转圈);
// 已加载单元缓存,回访秒开。数据层见 data.rs 的 DataStore。
#[component]
fn MainShell(token: Signal<Option<String>>) -> Element {
    let mut tab = use_signal(|| 0usize);
    let mut pending = use_signal(|| None::<usize>);
    let data = DataStore::provide(token);

    let go = move |i: usize| {
        if *tab.read() == i {
            return;
        }
        if pending.read() == Some(i) {
            return;
        }
        if data.all_ready(i) {
            pending.set(None);
            tab.set(i);
        } else {
            data.ensure_loaded(i);
            pending.set(Some(i));
        }
    };

    // 跨渲染稳定的跳转句柄(Overview 的「管理订阅源」按钮用)。
    let on_goto = use_callback(move |t: usize| go(t));

    // 当前页单元未加载(Idle)则自动预载;Error 不自动重试(由页内刷新按钮负责),避免死循环。
    if pending.read().is_none() && data.any_idle(*tab.read()) {
        data.ensure_loaded(*tab.read());
        pending.set(Some(*tab.read()));
    }

    // 加载完成提交切换:目标 tab 全部非 Loading(Ready/Error)时落定。
    use_effect(move || {
        let p = pending.read().clone();
        if let Some(p) = p {
            if data.all_finished(p) {
                tab.set(p);
                pending.set(None);
            }
        }
    });

    // pending 等于当前 tab 时(首次登录的默认页)无旧页可保持,渲染全页 loading。
    let content: Element = if pending.read() == Some(*tab.read()) {
        rsx! { div { class: "page-loading", Spinner { size: 28 } } }
    } else {
        match *tab.read() {
            0 => rsx! { Overview { token, on_goto } },
            1 => rsx! { Sources { token } },
            2 => rsx! { Combineds { token } },
            3 => rsx! { Preview { token } },
            _ => rsx! { Config { token } },
        }
    };

    rsx! {
        div { class: "app-shell",
            aside { class: "sidebar",
                div { class: "sidebar-brand",
                    {icon("logo", 22)}
                    span { "sub-merge" }
                }
                nav { class: "nav",
                    NavItem { name: "overview", label: "概览", active: *tab.read() == 0, loading: pending.read() == Some(0), onnav: move |_| go(0) }
                    NavItem { name: "sources", label: "订阅源", active: *tab.read() == 1, loading: pending.read() == Some(1), onnav: move |_| go(1) }
                    NavItem { name: "combineds", label: "组合订阅", active: *tab.read() == 2, loading: pending.read() == Some(2), onnav: move |_| go(2) }
                    NavItem { name: "preview", label: "预览", active: *tab.read() == 3, loading: pending.read() == Some(3), onnav: move |_| go(3) }
                    NavItem { name: "config", label: "配置", active: *tab.read() == 4, loading: pending.read() == Some(4), onnav: move |_| go(4) }
                }
                div { class: "sidebar-footer",
                    span { class: "sidebar-version", "v0.1.0" }
                    button { class: "btn btn-ghost btn-sm", onclick: move |_| {
                        clear_token();
                        token.set(None);
                    },
                        {icon("logout", 14)}
                        "退出登录"
                    }
                }
            }
            main { class: "main",
                div { class: "page-wrap",
                    {content}
                }
            }
        }
    }
}

#[component]
fn NavItem(
    name: &'static str,
    label: &'static str,
    active: bool,
    loading: bool,
    onnav: EventHandler<MouseEvent>,
) -> Element {
    rsx! {
        button { class: if active { "nav-item active" } else { "nav-item" }, onclick: onnav,
            {icon(name, 16)}
            span { "{label}" }
            if loading { Spinner { size: 12 } }
        }
    }
}
```

顶部 import 补 `use crate::data::DataStore;`(Spinner 已在 `use components::icon::{Spinner, icon};` 内)。

- [ ] **Step 3: index.html 加 .page-loading 样式**

在 `crates/server/web/index.html` 中 `.empty` 样式规则附近追加(参照现有 CSS 变量):

```css
.page-loading { display: flex; justify-content: center; padding: 80px 0; color: var(--muted); }
```

- [ ] **Step 4: 创建 ui-check.py 脚手架(含 nav_preload 场景)**

创建 `scripts/ui-check.py`(完整文件):

```python
#!/usr/bin/env python3
# 前端 UI 行为检查(CDP 驱动 headless chrome)。
# 前置:1) server 运行在 :18080(SUB_MERGE_ADMIN_TOKEN=test-token 预设)
#       2) chrome-headless-shell --headless --no-sandbox --remote-debugging-port=9222 \
#          --remote-allow-origins=* about:blank
# 用法:python3 scripts/ui-check.py <scenario> [url]
import json, sys, time, urllib.request, urllib.parse
import websocket

URL = sys.argv[2] if len(sys.argv) > 2 else "http://127.0.0.1:18080"
CDP = "http://127.0.0.1:9222"

def http_json(path, method="GET"):
    req = urllib.request.Request(CDP + path, method=method)
    with urllib.request.urlopen(req) as r:
        return json.loads(r.read())

def connect():
    target = http_json("/json/new?" + urllib.parse.quote(URL, safe=""), method="PUT")
    ws = websocket.create_connection(target["webSocketDebuggerUrl"], timeout=10)
    ws.settimeout(10)
    return ws

mid = [0]
def cmd(ws, method, params=None):
    mid[0] += 1
    ws.send(json.dumps({"id": mid[0], "method": method, "params": params or {}}))
    while True:
        msg = json.loads(ws.recv())
        if msg.get("id") == mid[0]:
            return msg.get("result", {})

def ev(ws, expr, timeout=6):
    ws.settimeout(timeout)
    try:
        return cmd(ws, "Runtime.evaluate", {"expression": expr, "returnByValue": True}).get("result", {}).get("value")
    except Exception:
        return ">>>TIMEOUT<<<"

def login(ws):
    cmd(ws, "Page.enable"); cmd(ws, "Runtime.enable")
    time.sleep(2)
    for _ in range(20):
        if ev(ws, "document.readyState") == "complete":
            break
        time.sleep(0.5)
    ev(ws, "localStorage.setItem('submerge_admin_token','test-token')")
    cmd(ws, "Page.reload")
    time.sleep(2.5)

def nav_el(ws, label):
    return ev(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('%s'))!==undefined" % label)

def nav_loading(ws, label):
    return ev(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('%s'))?.querySelector('.spinner')!==undefined" % label)

def nav_active(ws, label):
    return ev(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('%s')).classList.contains('active')" % label)

def click_nav(ws, label):
    ev(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('%s')).click()" % label)
    time.sleep(0.3)

def assert_true(cond, name):
    print(("PASS " if cond else "FAIL ") + name)
    if not cond:
        sys.exit(1)

def wait_until(ws, expr, timeout=20, interval=0.5):
    for _ in range(int(timeout / interval)):
        if ev(ws, expr):
            return True
        time.sleep(interval)
    return ev(ws, expr)

def seed_sources(ws, n):
    """经 API 种 n 个 single 源(需 server 已运行、token=test-token)。"""
    import urllib.request as u
    for i in range(n):
        req = u.Request(URL + "/admin/sources", method="POST",
                        data=json.dumps({"name": "s%d" % i,
                                         "url": "vless://e99a8e5a-6b2b-4a1d-9c5f-1a2b3c4d5e6f@1.2.3.%d:443#n%d" % (i + 1, i),
                                         "kind": "single"}).encode(),
                        headers={"Authorization": "Bearer test-token", "Content-Type": "application/json"})
        u.urlopen(req, timeout=5)

def scenario_nav_preload(ws):
    """首次切换:旧页保持 + 菜单项转圈 → 就绪后切换;已加载页回访秒开。"""
    seed_sources(ws, 1)
    login(ws)
    assert_true(nav_loading(ws, "概览"), "初始概览加载中菜单项转圈")
    assert_true(ev(ws, "!!document.querySelector('.page-loading')"), "初始加载内容区显示全页 loading")
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('概览')).classList.contains('active')"), "概览就绪后激活")
    # 回访秒开:订阅源单元已随概览预载 → 点击立即切换
    click_nav(ws, "订阅源")
    assert_true(nav_active(ws, "订阅源"), "已缓存单元切换秒开(无转圈)")
    assert_true(not nav_loading(ws, "订阅源"), "秒开路径无转圈")
    # 概览回访:数据缓存,秒开
    click_nav(ws, "概览")
    assert_true(nav_active(ws, "概览"), "概览回访秒开")
    assert_true(not nav_loading(ws, "概览"), "概览回访无转圈")

def main():
    scenario = sys.argv[1] if len(sys.argv) > 1 else "nav_preload"
    ws = connect()
    scenarios = {"nav_preload": scenario_nav_preload}
    scenarios[scenario](ws)
    print("== %s: ALL PASS ==" % scenario)

if __name__ == "__main__":
    main()
```

依赖:`python3-websocket`(apt 包,`apt-get install -y python3-websocket`)。

- [ ] **Step 5: 构建 + 跑 nav_preload 场景**

Run:
```bash
cd crates/server/web && dx build --web --release --debug-symbols false
# 起测试 server(新 DB + 固定 token;server 二进制为仓库根 target/debug/server):
rm -f /tmp/submerge-ui.db*; WEB_DIST=/root/github/sub-merge/crates/server/web/dist \
  DATABASE_PATH=/tmp/submerge-ui.db SUB_MERGE_ADMIN_TOKEN=test-token PORT=18080 \
  /root/github/sub-merge/target/debug/server > /tmp/ui-server.log 2>&1 &
# 起 chrome(如未运行;实际路径以本机为准):
/root/.cache/ms-playwright/chromium_headless_shell-1234/chrome-headless-shell-linux64/chrome-headless-shell --headless --no-sandbox --remote-debugging-port=9222 --remote-allow-origins=* about:blank &
cd /root/github/sub-merge && python3 scripts/ui-check.py nav_preload
```
Expected: dx 构建 0 警告;nav_preload 全部 PASS(注意:本任务中页面仍自带 use_future 拉取,导航行为断言不受影响)

- [ ] **Step 6: 门禁 + 提交**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets && cargo test --workspace
git add crates/server/web/src/data.rs crates/server/web/src/main.rs crates/server/web/index.html scripts/ui-check.py
git commit -m "feat(web): DataStore 单元缓存 + MainShell 预载编排(先载入再显示,菜单项转圈)"
```

---

### Task 3: Overview 切缓存

**Files:**
- Modify: `crates/server/web/src/components/overview.rs`

**Interfaces:**
- Consumes: `DataStore`(Task 2)、`CacheStatus`、`UnitKey`
- Produces: 无(页面内部改造)

- [ ] **Step 1: 删除本地拉取,改读缓存**

overview.rs 改动(逐处):
- import 区:删 `use crate::api::request;`、`use submerge_web_core::dto::PreviewSummary;`;`use crate::data::{fetch_preview_summary, fetch_sources};` 改为 `use crate::data::{CacheStatus, DataStore, UnitKey};`
- 删第 11-14 行 `fetch_preview` 函数
- 组件头三行信号(第 18-21 行 `let sources = ...; let stats = ...; let error = ...; let loading = ...;`)改为:

```rust
    let data = use_context::<DataStore>();
    let sources_state = data.sources.read().clone();
    let preview_state = data.preview.read().clone();
```

- 删 use_future 整块(第 24-43 行)
- `reload`(第 45-64 行)替换为:

```rust
    let refreshing = sources_state.status == CacheStatus::Loading
        || preview_state.status == CacheStatus::Loading;
    let reload = move |_| {
        data.refresh(UnitKey::Sources);
        data.refresh(UnitKey::Preview);
    };
```

- 统计值(第 67-73 行)替换为:

```rust
    let source_total = sources_state.data.as_ref().map(|s| s.len()).unwrap_or(0);
    let enabled_count = sources_state
        .data
        .as_ref()
        .map(|s| s.iter().filter(|s| s.enabled).count())
        .unwrap_or(0);
    let (node_total, failed_count) = preview_state
        .data
        .as_ref()
        .map(|s| (s.total, s.errors.len()))
        .unwrap_or((0, 0));
```

- source_rows(第 75-90 行):`sources.read().iter()` → `sources_state.data.as_ref().map(|list| list.iter().map(|s| {...原闭包体不变...}).collect()).unwrap_or_default()`
- error_rows(第 92-106 行):`stats.read().as_ref()` → `preview_state.data.as_ref()`(其余不变)
- 错误展示:原 `if !error.read().is_empty() { p { class: "error-text", "{error}" } }` 改为:

```rust
        let page_error = [sources_state.error.clone(), preview_state.error.clone()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("; ");
```

rsx 内 `if !page_error.is_empty() { p { class: "error-text", "{page_error}" } }`
- 刷新按钮(原 `disabled: *loading.read()` 与 `if *loading.read()`):`loading` 引用全部换成 `refreshing`(disabled 与 Spinner 分支)

- [ ] **Step 2: 构建 + 场景验证**

```bash
cd crates/server/web && dx build --web --release --debug-symbols false
cd /root/github/sub-merge && python3 scripts/ui-check.py nav_preload
```
Expected: 0 警告;nav_preload 仍全 PASS(概览数据此时来自缓存——若断言仍过,说明缓存链路通)

- [ ] **Step 3: 提交**

```bash
git add crates/server/web/src/components/overview.rs
git commit -m "feat(web): 概览页改读 DataStore 缓存(统计/最近错误/刷新)"
```

---

### Task 4: Sources 切缓存 + CRUD 回写

**Files:**
- Modify: `crates/server/web/src/components/sources.rs`

**Interfaces:**
- Consumes: `DataStore`、`UnitKey`、`CacheStatus`(视需要)
- Produces: 无

- [ ] **Step 1: 数据源改缓存**

sources.rs 改动:
- import:删 `use crate::data::fetch_sources;`;加 `use crate::data::{DataStore, UnitKey};`
- 删第 18 行 `let sources = use_signal(Vec::<SourceDto>::new);`;组件头加 `let data = use_context::<DataStore>();`
- 删 use_future 整块(第 30-40 行)
- 在组件头(use_future 原位置)加快照:

```rust
    let sources_state = data.sources.read().clone();
    let source_list = sources_state.data.unwrap_or_default();
```

- 行渲染(第 176-223 行):`sources.read().iter()` → `source_list.iter()`(其余不变)
- ask_delete 名字查找(第 134-139 行):`sources.read().iter()` → `source_list.iter()`
- error 展示区:表单错误与缓存错误合并,rsx 内 `if !error.read().is_empty() { p { class: "error-text", "{error}" } }` 改为:

```rust
        let page_error = if error.read().is_empty() {
            sources_state.error.clone()
        } else {
            error.read().clone()
        };
```

rsx 内 `if !page_error.is_empty() { p { class: "error-text", "{page_error}" } }`

- [ ] **Step 2: CRUD 成功后回写缓存**

三处 `fetch_sources(token.as_deref()).await` 的重拉替换为缓存刷新:
- add 闭包(第 62 行 `Ok(list) => { sources.set(list); ...`):改为

```rust
                    Ok(_) => {
                        data.refresh(UnitKey::Sources);
                        new_url.set(String::new());
                        new_name.set(String::new());
                        error.set(String::new());
                        push_toast(toasts, ToastKind::Success, "订阅源已添加");
                    }
```

(add 闭包内原 `let mut sources = sources.clone();` 一并删除;fetch_sources 内层 match 删除——POST 成功即 refresh,失败由外层 Err 分支处理)
- toggle 闭包(第 87-97 行):`match fetch_sources(...)` 整块替换为 `data.refresh(UnitKey::Sources); push_toast(toasts, ToastKind::Info, ...);`(原 Ok 分支的两步合并;删 `let mut sources = sources.clone();`)
- on_confirm_delete(第 158-171 行):DELETE Ok 后 `match fetch_sources(...)` 整块替换为 `data.refresh(UnitKey::Sources); push_toast(toasts, ToastKind::Success, "已删除");`(删 `let mut sources = sources.clone();`)
- 行刷新 refresh(id) 闭包不变(POST refresh 只发 toast,不重拉列表)

- [ ] **Step 3: 构建 + 场景验证**

```bash
cd crates/server/web && dx build --web --release --debug-symbols false
cd /root/github/sub-merge && python3 scripts/ui-check.py nav_preload
```
Expected: 0 警告;nav_preload 全 PASS。

在 ui-check.py 追加 `scenario_sources_crud` 并跑通:

```python
def scenario_sources_crud(ws):
    """订阅源页添加源 → 切概览 → 统计同步(缓存回写)。"""
    login(ws)
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('概览')).classList.contains('active')"), "概览就绪")
    click_nav(ws, "订阅源")
    time.sleep(0.5)
    # 添加表单:kind 下拉 + URL + 名称 两个 input + 添加按钮(以实际 DOM 为准,先打印结构)
    print(ev(ws, "document.querySelector('.form-row')?.innerText.slice(0,200)"))
    ev(ws, "(()=>{const ins=document.querySelectorAll('.form-row input');ins[0].value='vless://e99a8e5a-6b2b-4a1d-9c5f-1a2b3c4d5e6f@9.9.9.9:443#crud-test';ins[0].dispatchEvent(new Event('input',{bubbles:true}));ins[1].value='crud-test';ins[1].dispatchEvent(new Event('input',{bubbles:true}));})()")
    ev(ws, "Array.from(document.querySelectorAll('.form-row button')).find(b=>b.textContent.includes('添加')).click()")
    time.sleep(0.8)
    assert_true(wait_until(ws, "document.body.innerText.includes('crud-test')"), "添加后列表出现新源")
    click_nav(ws, "概览")
    time.sleep(0.3)
    assert_true(ev(ws, "document.querySelector('.stat-value')?.textContent === '2'"), "概览源总数同步为 2(缓存回写)")
```

(向 `scenarios` dict 注册;若表单 input 顺序与假设不符,以 Step 1 打印的实际 DOM 调整索引)

- [ ] **Step 4: 门禁 + 提交**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets && cargo test --workspace
git add crates/server/web/src/components/sources.rs scripts/ui-check.py
git commit -m "feat(web): 订阅源页改读缓存,CRUD 后 refresh 回写(概览同步)"
```

---

### Task 5: Combineds 切缓存 + CRUD 回写

**Files:**
- Modify: `crates/server/web/src/components/combineds.rs`

**Interfaces:**
- Consumes: `DataStore`、`UnitKey`
- Produces: 无

- [ ] **Step 1: 数据源改缓存**

combineds.rs 改动:
- import:删 `use crate::data::{fetch_combineds, fetch_sources};`;加 `use crate::data::{DataStore, UnitKey};`(第 7 行 SourceDto 保留——member_rows 用)
- 删第 30-31 行 `let combineds = ...; let sources = ...;`;组件头加 `let data = use_context::<DataStore>();`
- 删 use_future 整块(第 42-58 行)
- 组件头加快照:

```rust
    let combineds_state = data.combineds.read().clone();
    let combined_list = combineds_state.data.unwrap_or_default();
    let sources_state = data.sources.read().clone();
    let source_list = sources_state.data.unwrap_or_default();
```

- 行渲染(第 239-284 行):`combineds.read().iter()` → `combined_list.iter()`
- member_rows(第 211-236 行):`sources.read().iter()` → `source_list.iter()`
- open_edit 查找(第 72 行):`combineds.read().iter()` → `combined_list.iter()`
- ask_delete 名字查找(第 144-149 行):`combineds.read().iter()` → `combined_list.iter()`
- 错误展示:表单错误优先、缓存错误兜底(与 Task 4 相同模式):

```rust
        let page_error = if error.read().is_empty() {
            combineds_state.error.clone()
        } else {
            error.read().clone()
        };
```

rsx 内 `if !page_error.is_empty() { p { class: "error-text", "{page_error}" } }`

- [ ] **Step 2: CRUD 成功后回写缓存**

- save 闭包(第 127-135 行):`match fetch_combineds(token.as_deref()).await { Ok(list) => { combineds.set(list); form.set(...); error.set(...); push_toast(...) } ...` 改为:

```rust
            match result {
                Ok(_) => {
                    data.refresh(UnitKey::Combineds);
                    form.set(FormState::default());
                    error.set(String::new());
                    push_toast(toasts, ToastKind::Success, "组合订阅已保存");
                }
                Err(e) => error.set(format!("保存失败: {e}")),
            }
            saving.set(false);
```

(删 `let mut combineds = combineds.clone();`)
- on_confirm_delete(第 175-184 行):DELETE Ok 后 `match fetch_combineds(...)` 整块替换为 `data.refresh(UnitKey::Combineds); push_toast(toasts, ToastKind::Success, "已删除");`(删 `let mut combineds = combineds.clone();`)

- [ ] **Step 3: 构建 + 场景验证**

```bash
cd crates/server/web && dx build --web --release --debug-symbols false
cd /root/github/sub-merge && python3 scripts/ui-check.py nav_preload
```
Expected: 0 警告;nav_preload 全 PASS。

ui-check.py 追加 `scenario_combineds` 并跑通:

```python
def scenario_combineds(ws):
    """组合订阅:新建 → 列表出现;保存/删除后缓存刷新。"""
    seed_sources(ws, 1)
    login(ws)
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('概览')).classList.contains('active')"), "概览就绪")
    click_nav(ws, "组合订阅")
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('组合订阅')).classList.contains('active')"), "组合订阅就绪")
    ev(ws, "Array.from(document.querySelectorAll('button')).find(b=>b.textContent.includes('新建组合')).click()")
    time.sleep(0.5)
    ev(ws, "(()=>{const el=document.querySelector('.modal input');const s=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set;s.call(el,'c-test');el.dispatchEvent(new Event('input',{bubbles:true}));})()")
    ev(ws, "document.querySelector('.member-row input').click()")
    time.sleep(0.3)
    ev(ws, "Array.from(document.querySelectorAll('.modal-actions button')).find(b=>b.textContent.includes('保存')).click()")
    assert_true(wait_until(ws, "document.body.innerText.includes('c-test')"), "保存后列表出现 c-test")
```

(注册进 `scenarios` dict 后跑 `python3 scripts/ui-check.py combineds`)

- [ ] **Step 4: 门禁 + 提交**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets && cargo test --workspace
git add crates/server/web/src/components/combineds.rs scripts/ui-check.py
git commit -m "feat(web): 组合订阅页改读缓存,保存/删除后 refresh 回写"
```

---

### Task 6: Preview 切缓存 + 过滤本地化

**Files:**
- Modify: `crates/server/web/src/components/preview.rs`

**Interfaces:**
- Consumes: `DataStore`、`UnitKey`、`CacheStatus`、`data::fetch_preview` 不需要(过滤走本地 request)
- Produces: 无

- [ ] **Step 1: 初始数据改缓存,过滤本地化**

preview.rs 改动:
- import:删 `use crate::data::fetch_combineds;`;加 `use crate::data::{CacheStatus, DataStore, UnitKey};`(request/PreviewResp 保留)
- 第 14-19 行信号区改为:

```rust
    let data = use_context::<DataStore>();
    // 全部源视图来自 preview 缓存单元;按组合过滤的视图为页面本地状态。
    let local_data = use_signal(|| None::<PreviewResp>);
    let local_loading = use_signal(|| false);
    let local_error = use_signal(String::new);
    let mut selected = use_signal(|| None::<String>);
```

- `load_preview` use_callback(第 23-44 行)替换为:

```rust
    let load_preview = use_callback(move |selected: Option<String>| {
        if selected.is_none() {
            data.refresh(UnitKey::Preview);
            return;
        }
        let token = token.read().clone();
        let mut local_data = local_data.clone();
        let mut local_loading = local_loading.clone();
        let mut local_error = local_error.clone();
        let name = selected.unwrap();
        spawn(async move {
            local_loading.set(true);
            local_error.set(String::new());
            let path = format!("/admin/preview?combined={name}");
            match request("GET", &path, None, token.as_deref()).await {
                Ok(body) => match serde_json::from_str::<PreviewResp>(&body) {
                    Ok(r) => local_data.set(Some(r)),
                    Err(e) => local_error.set(format!("解析失败: {}", e)),
                },
                Err(e) => local_error.set(e.to_string()),
            }
            local_loading.set(false);
        });
    });
```

- 删 use_future 整块(第 47-58 行)
- 数据快照与派生(第 64 行 `let resp = data.read().clone();` 前插入):

```rust
    let preview_state = data.preview.read().clone();
    let combineds_state = data.combineds.read().clone();
    let resp = if selected.read().is_some() {
        local_data.read().clone()
    } else {
        preview_state.data.clone()
    };
```

- combined_options(第 104-114 行):`combineds.read().iter()` → `combineds_state.data.as_ref().map(|list| list.iter().map(|c| {...原闭包体不变...}).collect()).unwrap_or_default()`
- 刷新按钮(第 134-141 行):`disabled: *loading.read()` 与 `if *loading.read()` 换为:

```rust
            let busy = if selected.read().is_some() {
                *local_loading.read()
            } else {
                preview_state.status == CacheStatus::Loading
            };
```

`onclick: reload` 保留,`reload`(第 60-62 行)保持 `load_preview(selected.read().clone());`
- 错误展示:`if !error.read().is_empty() { p { class: "error-text", "{error}" } }` 改为:

```rust
        let page_error = if selected.read().is_some() {
            local_error.read().clone()
        } else {
            preview_state.error.clone()
        };
```

rsx 内 `if !page_error.is_empty() { p { class: "error-text", "{page_error}" } }`(local_error 为 String,直接显示)

- [ ] **Step 2: 构建 + 场景验证**

```bash
cd crates/server/web && dx build --web --release --debug-symbols false
cd /root/github/sub-merge && python3 scripts/ui-check.py nav_preload && python3 scripts/ui-check.py combineds
```
Expected: 0 警告;两场景全 PASS。

ui-check.py 追加 `scenario_preview_filter` 并跑通:

```python
def scenario_preview_filter(ws):
    """预览页:全部源视图来自缓存;过滤下拉走本地拉取。"""
    seed_sources(ws, 2)
    login(ws)
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('概览')).classList.contains('active')"), "概览就绪")
    click_nav(ws, "预览")
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('预览')).classList.contains('active')"), "预览就绪")
    assert_true(wait_until(ws, "document.querySelectorAll('.table-wrap tbody tr').length === 2"), "全部源视图 2 个节点")
    # 切过滤下拉(需先有组合:c-test 由 combineds 场景创建;若无,先经 API 建)
    ev(ws, "(()=>{const sel=document.querySelector('.preview-filter');const opts=[...sel.options];const t=opts.find(o=>o.textContent==='c-test');if(t){sel.value=t.value;sel.dispatchEvent(new Event('change',{bubbles:true}));}})()")
    time.sleep(0.8)
    assert_true(ev(ws, "document.querySelectorAll('.table-wrap tbody tr').length === 1"), "过滤视图只显示该组合成员")
```

(若无 c-test 组合,先在 scenario 开头经 API POST /admin/combineds 创建含 1 个成员的组合)

- [ ] **Step 3: 门禁 + 提交**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets && cargo test --workspace
git add crates/server/web/src/components/preview.rs scripts/ui-check.py
git commit -m "feat(web): 预览页初始数据走缓存,组合过滤本地化"
```

---

### Task 7: Config 切缓存 + 轮换回写

**Files:**
- Modify: `crates/server/web/src/components/config.rs`

**Interfaces:**
- Consumes: `DataStore`、`CacheStatus`、`data::fetch_config`
- Produces: 无

- [ ] **Step 1: 数据源改缓存**

config.rs 改动:
- import:`use crate::data::{fetch_config, CacheStatus, DataStore};`
- 删第 14 行 `let cfg = use_signal(|| None::<ConfigDto>);`;组件头加:

```rust
    let data = use_context::<DataStore>();
```

- 删 use_future 整块(第 22-35 行,Task 1 已改为 fetch_config 调用)
- admin_token_show(第 85-95 行):`cfg.read().as_ref()` → `data.config.read().data.as_ref()`(mask 逻辑不变)
- 轮换 rotate 闭包(第 37-62 行):Ok(b) 解析成功后,原 `cfg.set(Some(c));` 改为回写缓存:

```rust
                        let mut sig = data.config;
                        sig.set(CacheState { status: CacheStatus::Ready, data: Some(c.clone()), error: String::new() });
```

(需 `use crate::data::CacheState;`;write_token/token.set 保留)
- 错误展示:轮换错误走现有本地 `error` 信号;缓存 Error 兜底(与 Task 4 模式相同,`cfg` 相关引用改为 `data.config`)

- [ ] **Step 2: 构建 + 场景验证**

```bash
cd crates/server/web && dx build --web --release --debug-symbols false
cd /root/github/sub-merge && python3 scripts/ui-check.py nav_preload
```
Expected: 0 警告;nav_preload 全 PASS。

ui-check.py 追加 `scenario_config_rotate` 并跑通:

```python
def scenario_config_rotate(ws):
    """配置页:token 显示来自缓存;轮换后回写缓存 + 会话同步。"""
    login(ws)
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('概览')).classList.contains('active')"), "概览就绪")
    click_nav(ws, "配置")
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('配置')).classList.contains('active')"), "配置就绪")
    old = ev(ws, "document.querySelector('.token-value')?.textContent")
    ev(ws, "Array.from(document.querySelectorAll('button')).find(b=>b.textContent.includes('轮换')).click()")
    time.sleep(0.5)
    ev(ws, "Array.from(document.querySelectorAll('.modal-actions button')).find(b=>b.textContent.includes('轮换')).click()")
    assert_true(wait_until(ws, "document.querySelector('.token-value')?.textContent !== '%s'" % old), "轮换后 token 显示已更新(缓存回写)")
```

- [ ] **Step 3: 门禁 + 提交**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets && cargo test --workspace
git add crates/server/web/src/components/config.rs scripts/ui-check.py
git commit -m "feat(web): 配置页改读缓存,轮换后回写 config 单元"
```

---

### Task 8: 全量验证 + CLAUDE.md 补注

**Files:**
- Modify: `CLAUDE.md`
- Modify(仅当发现缺陷):各页面文件

- [ ] **Step 1: 全场景回归**

```bash
# 清理旧测试进程后:
rm -f /tmp/submerge-ui.db*
cd /root/github/sub-merge
python3 scripts/ui-check.py nav_preload
python3 scripts/ui-check.py sources_crud
python3 scripts/ui-check.py combineds
python3 scripts/ui-check.py preview_filter
python3 scripts/ui-check.py config_rotate
```
Expected: 全部 ALL PASS。

- [ ] **Step 2: make smoke**

Run: `make smoke`
Expected: 9/9 全部通过,无 SIGABRT,served wasm 为 766KB 级优化产物。

- [ ] **Step 3: cargo 门禁**

```bash
cargo upgrade -i
cargo fmt --all
cargo clippy --workspace --all-targets
cargo test --workspace
```
Expected: 全部通过(clippy 0 警告,测试套件全绿)。

- [ ] **Step 4: CLAUDE.md 架构段补注**

`CLAUDE.md` 的 `crates/server/web` 架构条目末尾追加一句:

```
页面数据经 `src/data.rs` 的 DataStore 单元缓存(按 sources/combineds/preview/config 四单元共享):MainShell 先预载再切换 tab(旧页保持+菜单项转圈),已访问页缓存秒开;CRUD/刷新后 `refresh` 对应单元回写缓存。
```

- [ ] **Step 5: 提交**

```bash
git add CLAUDE.md
git commit -m "docs: web crate 数据层(DataStore 缓存)架构注记"
```
