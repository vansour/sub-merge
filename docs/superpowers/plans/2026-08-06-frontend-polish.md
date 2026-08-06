# 前端美化实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 sub-merge 管理界面从简朴浅色页面重做为专业双主题（深/浅自动切换）管理面板：侧边栏导航、统计概览、SVG 图标、Toast/确认弹窗/加载态、响应式布局。

**Architecture:** 纯前端改造（`crates/server/web`）。CSS 全部在 `index.html` 内联（CSS 变量双主题）；新增 4 个组件模块（icon/toast/confirm/overview）；改造 5 个已有组件（login/sources/preview/config + main.rs 主壳）。零后端改动、零新增依赖。

**Tech Stack:** Rust 1.97+ / dioxus 0.8.0-alpha.1 / dx 0.8.0-alpha.1 / WASM / 手写 CSS（CSS 变量 + `prefers-color-scheme` + `color-mix`）

**测试说明（重要）:** web crate 是 WASM UI，无测试 harness（与 proxy-core/server 不同）。本计划用**构建验证 + 浏览器人工验证**替代 TDD：每个任务以 `dx build --web --release` 成功为验收门槛，Task 11 做全量浏览器验证清单。这是本项目既有惯例（smoke 脚本只覆盖静态/API）。

## Global Constraints

- 依赖锁定：dioxus `0.8.0-alpha.1`、dx `0.8.0-alpha.1`、Rust 1.97+、edition 2024 —— **不得升级/降级**
- **零新增依赖**：不得修改 `crates/server/web/Cargo.toml` 的 `[dependencies]`
- **零后端改动**：`crates/server`、`crates/proxy-core` 一律不碰
- UI 文案一律中文；CSS 类名与变量必须使用 Task 1 定义的，不得自创新类名（如需新增须在 Task 1 的 CSS 内补充定义）
- 提交信息遵循仓库风格：`feat(web):` / `style(web):` / `fix(web):`，结尾带 `Co-Authored-By: Claude <noreply@anthropic.com>`
- 每个任务收尾必须 `cd crates/server/web && dx build --web --release` 通过（首次构建因 wasm 依赖编译较慢，属正常）
- 已知 dioxus 0.8 alpha 注意事项：`use_context_provider(f)` 的 `T: 'static + Clone`；`use_effect` 无依赖数组（每次渲染都跑，需要自守卫）；`EventHandler<T> = Callback<T>`，可用 `use_callback` 创建（避免在 render 体直接 `EventHandler::new` 累积泄漏）；`spawn` 可在事件处理器内直接调用

## File Structure

| 文件 | 责任 | 任务 |
|------|------|------|
| `crates/server/web/index.html` | CSS 设计系统（双主题 tokens + 全部组件类） | T1 |
| `crates/server/web/src/components/icon.rs` | 内联 SVG 图标 + Spinner | T2 |
| `crates/server/web/src/components/toast.rs` | Toast 全局上下文 + 自动消失 + schedule_timeout 工具 | T3 |
| `crates/server/web/src/components/confirm.rs` | 确认弹窗 | T4 |
| `crates/server/web/src/components/login.rs` | 登录页（卡片式 + 密码输入 + Enter 提交） | T5 |
| `crates/server/web/src/components/overview.rs` | 概览页（统计卡片 + 摘要） | T6 |
| `crates/server/web/src/components/sources.rs` | 订阅源 CRUD（SourceDto + fetch_sources 共享） | T6 加函数 / T8 重写 |
| `crates/server/web/src/main.rs` | 主壳（侧边栏 + ToastProvider + tab 切换） | T7 |
| `crates/server/web/src/components/preview.rs` | 转换预览（协议徽章 + 错误卡片） | T9 |
| `crates/server/web/src/components/config.rs` | 配置页（链接卡片 + token 掩码 + 轮换确认） | T10 |

接口契约（后续任务依赖）：

```rust
// icon.rs
pub fn icon(name: &'static str, size: u32) -> Element
// name 合法值："logo" "overview" "sources" "preview" "config" "logout" "refresh" "copy" "trash" "plus" "check" "x" "alert"；未知名渲染空 span
#[component] pub fn Spinner(size: u32) -> Element   // 用法：Spinner { size: 14 }

// toast.rs
#[derive(Debug, Clone, Copy, PartialEq)] pub enum ToastKind { Success, Error, Info }
#[derive(Debug, Clone, PartialEq)] pub struct ToastMsg { pub id: u64, pub kind: ToastKind, pub text: String }
#[component] pub fn ToastProvider() -> Element          // 提供 context 并渲染堆叠，须包在需要 toast 的子树外层
pub fn use_toast() -> Signal<Vec<ToastMsg>>             // 只在 ToastProvider 子树内的组件可调用
pub fn push_toast(mut toasts: Signal<Vec<ToastMsg>>, kind: ToastKind, text: impl Into<String>)
pub(crate) fn schedule_timeout(ms: u32, f: impl FnOnce() + 'static)  // wasm setTimeout 封装

// confirm.rs
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ConfirmState { pub open: bool, pub title: String, pub message: String, pub confirm_text: String, pub danger: bool }
#[component] pub fn ConfirmDialog(state: Signal<ConfirmState>, on_confirm: EventHandler<()>) -> Element

// sources.rs
pub struct SourceDto { /* 现有字段不变 */ }
pub async fn fetch_sources(token: Option<&str>) -> Result<Vec<SourceDto>, String>

// overview.rs
#[component] pub fn Overview(token: Signal<Option<String>>, on_goto: EventHandler<usize>) -> Element
```

---

### Task 1: CSS 设计系统（index.html 重写）

**Files:**
- Rewrite: `crates/server/web/index.html`

**Interfaces:**
- Produces: 全部 CSS 类名与变量（后续所有任务消费）。类名清单见下方 CSS 注释。

- [ ] **Step 1: 重写 index.html 的 `<style>` 块**

将现有 `<style>`（body/.container/.card/button/input/table/.badge 等）整体替换为以下设计系统（`<head>` 其余部分与 `<body>` 的 `<div id="main">` 保持不变）：

```html
<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>sub-merge 管理</title>
  <style>
    /* ========== 设计 tokens：浅色 ========== */
    :root {
      --bg: #f6f7f9;
      --bg-elevated: #ffffff;
      --bg-soft: #eef0f3;
      --card: #ffffff;
      --text: #17181c;
      --text-secondary: #5f6672;
      --text-tertiary: #9ca3af;
      --border: #e4e7ec;
      --accent: #2563eb;
      --accent-hover: #1d4ed8;
      --accent-contrast: #ffffff;
      --accent-soft: #e8f0fe;
      --success: #16a34a;
      --success-soft: #e9f9ef;
      --danger: #dc2626;
      --danger-soft: #fdecec;
      --danger-btn: #dc2626;
      --danger-btn-hover: #b91c1c;
      --warning: #b45309;
      --warning-soft: #fdf3e3;
      --shadow-card: 0 1px 2px rgba(16,24,40,.05), 0 1px 3px rgba(16,24,40,.06);
      --font-mono: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
      --proto-0-bg: #dcfce7; --proto-0-fg: #15803d;
      --proto-1-bg: #dbeafe; --proto-1-fg: #1d4ed8;
      --proto-2-bg: #ede9fe; --proto-2-fg: #6d28d9;
      --proto-3-bg: #ffedd5; --proto-3-fg: #c2410c;
      --proto-4-bg: #fce7f3; --proto-4-fg: #be185d;
      --proto-5-bg: #ccfbf1; --proto-5-fg: #0f766e;
    }
    /* ========== 设计 tokens：深色（跟随系统） ========== */
    @media (prefers-color-scheme: dark) {
      :root {
        --bg: #0f1115;
        --bg-elevated: #13151b;
        --bg-soft: #1d2128;
        --card: #17191f;
        --text: #e6e8ee;
        --text-secondary: #9aa1ad;
        --text-tertiary: #636b78;
        --border: #262a33;
        --accent: #38bdf8;
        --accent-hover: #7dd3fc;
        --accent-contrast: #0b1220;
        --accent-soft: #13253f;
        --success: #4ade80;
        --success-soft: #12301e;
        --danger: #f87171;
        --danger-soft: #331c1e;
        --danger-btn: #932727;
        --danger-btn-hover: #a83434;
        --warning: #fbbf24;
        --warning-soft: #322712;
        --shadow-card: 0 1px 2px rgba(0,0,0,.4), 0 1px 3px rgba(0,0,0,.5);
        --proto-0-bg: #10291b; --proto-0-fg: #6ee7a0;
        --proto-1-bg: #12233d; --proto-1-fg: #7dd3fc;
        --proto-2-bg: #231c3e; --proto-2-fg: #c4b5fd;
        --proto-3-bg: #291c10; --proto-3-fg: #fdba74;
        --proto-4-bg: #2c1626; --proto-4-fg: #f9a8d4;
        --proto-5-bg: #0e2a26; --proto-5-fg: #5eead4;
      }
    }

    /* ========== 基础 ========== */
    * { box-sizing: border-box; }
    body {
      margin: 0; padding: 0;
      font-family: system-ui, -apple-system, "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif;
      background: var(--bg); color: var(--text);
      font-size: 14px; -webkit-font-smoothing: antialiased;
      transition: background-color .15s ease, color .15s ease;
    }
    .mono { font-family: var(--font-mono); font-size: 13px; }

    /* ========== 布局：侧边栏 + 主内容 ========== */
    .app-shell { display: flex; min-height: 100vh; }
    .sidebar {
      width: 220px; flex-shrink: 0; height: 100vh; position: sticky; top: 0;
      background: var(--bg-elevated); border-right: 1px solid var(--border);
      display: flex; flex-direction: column; padding: 16px 12px; gap: 4px;
    }
    .sidebar-brand { display: flex; align-items: center; gap: 10px; padding: 4px 10px 16px; font-size: 16px; font-weight: 600; }
    .sidebar-brand svg { color: var(--accent); }
    .nav { display: flex; flex-direction: column; gap: 2px; flex: 1; }
    .nav-item {
      display: flex; align-items: center; gap: 10px; width: 100%;
      padding: 9px 10px; border: none; border-radius: 8px; background: none;
      color: var(--text-secondary); font-size: 14px; font-family: inherit; cursor: pointer; text-align: left;
      transition: background-color .15s ease, color .15s ease;
    }
    .nav-item:hover { background: var(--bg-soft); color: var(--text); }
    .nav-item.active { background: var(--accent-soft); color: var(--accent); font-weight: 500; }
    .sidebar-footer { border-top: 1px solid var(--border); padding-top: 12px; display: flex; flex-direction: column; gap: 8px; }
    .sidebar-version { font-size: 12px; color: var(--text-tertiary); padding: 0 10px; }
    .main { flex: 1; min-width: 0; padding: 28px 32px; }
    .page-wrap { max-width: 960px; margin: 0 auto; }
    .page-head { display: flex; align-items: center; justify-content: space-between; margin-bottom: 20px; gap: 12px; }
    .page-title { font-size: 20px; font-weight: 600; margin: 0; }

    /* ========== 按钮 ========== */
    .btn {
      display: inline-flex; align-items: center; gap: 6px;
      border: none; border-radius: 6px; padding: 8px 14px;
      font-size: 14px; font-weight: 500; font-family: inherit; cursor: pointer;
      transition: background-color .15s ease, color .15s ease, opacity .15s ease;
    }
    .btn:disabled { opacity: .55; cursor: not-allowed; }
    .btn:focus-visible, .nav-item:focus-visible, input:focus-visible {
      outline: 2px solid color-mix(in srgb, var(--accent) 50%, transparent); outline-offset: 2px;
    }
    .btn-primary { background: var(--accent); color: var(--accent-contrast); }
    .btn-primary:hover { background: var(--accent-hover); }
    .btn-primary:disabled:hover { background: var(--accent); }
    .btn-secondary { background: var(--bg-soft); color: var(--text); }
    .btn-secondary:hover { background: var(--border); }
    .btn-danger { background: var(--danger-btn); color: #fff; }
    .btn-danger:hover { background: var(--danger-btn-hover); }
    .btn-ghost { background: transparent; color: var(--text-secondary); border: 1px solid var(--border); }
    .btn-ghost:hover { background: var(--bg-soft); color: var(--text); }
    .btn-sm { padding: 4px 10px; font-size: 13px; border-radius: 6px; }
    .btn.checked { background: var(--success-soft); color: var(--success); border-color: transparent; }

    /* ========== 表单 ========== */
    .form-row { display: flex; gap: 12px; align-items: flex-end; }
    .field { display: flex; flex-direction: column; gap: 4px; flex: 1; min-width: 0; }
    .field label { font-size: 12px; color: var(--text-secondary); font-weight: 500; }
    .field input {
      width: 100%; background: var(--card); border: 1px solid var(--border); border-radius: 6px;
      padding: 8px 10px; font-size: 14px; color: var(--text); font-family: inherit;
      transition: border-color .15s ease, box-shadow .15s ease;
    }
    .field input::placeholder { color: var(--text-tertiary); }
    .field input:focus {
      outline: none; border-color: var(--accent);
      box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 18%, transparent);
    }
    .error-text { font-size: 12.5px; color: var(--danger); margin: 4px 0 0; }

    /* ========== 卡片 ========== */
    .card { background: var(--card); border: 1px solid var(--border); border-radius: 10px; padding: 20px; box-shadow: var(--shadow-card); }
    .card + .card { margin-top: 16px; }
    .card-title { font-size: 15px; font-weight: 600; margin: 0 0 14px; }
    .card-foot { margin-top: 12px; display: flex; justify-content: flex-end; }
    .grid-2 { display: grid; grid-template-columns: 1fr 1fr; gap: 16px; }
    .subtle { font-size: 12.5px; color: var(--text-secondary); margin: 0 0 10px; line-height: 1.5; }

    /* ========== 统计卡片 ========== */
    .stats-grid { display: grid; grid-template-columns: repeat(4, 1fr); gap: 16px; margin-bottom: 16px; }
    .stat-card { background: var(--card); border: 1px solid var(--border); border-radius: 10px; padding: 16px; box-shadow: var(--shadow-card); display: flex; align-items: center; gap: 12px; }
    .stat-icon { width: 40px; height: 40px; border-radius: 10px; background: var(--accent-soft); color: var(--accent); display: flex; align-items: center; justify-content: center; flex-shrink: 0; }
    .stat-icon.danger { background: var(--danger-soft); color: var(--danger); }
    .stat-value { font-size: 26px; font-weight: 600; line-height: 1.1; }
    .stat-label { font-size: 12px; color: var(--text-secondary); margin-top: 2px; }

    /* ========== 表格 ========== */
    .table-wrap { overflow-x: auto; }
    table { width: 100%; border-collapse: collapse; font-size: 13px; }
    th { text-align: left; font-size: 12px; font-weight: 500; color: var(--text-secondary); padding: 10px 12px; border-bottom: 1px solid var(--border); white-space: nowrap; }
    td { padding: 10px 12px; border-bottom: 1px solid var(--border); vertical-align: middle; }
    tbody tr { transition: background-color .1s ease; }
    tbody tr:hover { background: var(--bg-soft); }
    .cell-name { font-weight: 500; white-space: nowrap; }
    .cell-url {
      font-family: var(--font-mono); font-size: 12.5px; color: var(--text-secondary);
      max-width: 300px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    }
    .actions { display: flex; gap: 6px; justify-content: flex-end; }

    /* ========== 徽章 ========== */
    .badge { display: inline-flex; align-items: center; gap: 5px; padding: 2px 10px; border-radius: 999px; font-size: 12px; font-weight: 500; }
    .badge::before { content: ""; width: 6px; height: 6px; border-radius: 50%; background: currentColor; }
    .badge.on { background: var(--success-soft); color: var(--success); }
    .badge.off { background: var(--bg-soft); color: var(--text-secondary); }
    .proto { display: inline-block; padding: 2px 8px; border-radius: 6px; font-family: var(--font-mono); font-size: 11.5px; }
    .proto-0 { background: var(--proto-0-bg); color: var(--proto-0-fg); }
    .proto-1 { background: var(--proto-1-bg); color: var(--proto-1-fg); }
    .proto-2 { background: var(--proto-2-bg); color: var(--proto-2-fg); }
    .proto-3 { background: var(--proto-3-bg); color: var(--proto-3-fg); }
    .proto-4 { background: var(--proto-4-bg); color: var(--proto-4-fg); }
    .proto-5 { background: var(--proto-5-bg); color: var(--proto-5-fg); }

    /* ========== Toast ========== */
    .toast-stack { position: fixed; top: 16px; right: 16px; z-index: 100; display: flex; flex-direction: column; gap: 8px; pointer-events: none; }
    .toast {
      pointer-events: auto; display: flex; align-items: center; gap: 10px;
      background: var(--card); border: 1px solid var(--border); border-radius: 8px;
      padding: 10px 14px; font-size: 13.5px; max-width: 360px;
      box-shadow: 0 4px 12px rgba(0,0,0,.15); animation: toast-in .15s ease-out;
    }
    .toast.success { border-left: 3px solid var(--success); }
    .toast.error { border-left: 3px solid var(--danger); }
    .toast.info { border-left: 3px solid var(--accent); }
    @keyframes toast-in { from { opacity: 0; transform: translateY(-8px); } to { opacity: 1; transform: none; } }
    .toast-close { border: none; background: none; color: var(--text-tertiary); cursor: pointer; font-size: 14px; padding: 0 0 0 6px; margin-left: auto; display: flex; }

    /* ========== 确认弹窗 ========== */
    .modal-overlay { position: fixed; inset: 0; background: rgba(0,0,0,.45); display: flex; align-items: center; justify-content: center; z-index: 200; padding: 16px; }
    .modal { background: var(--card); border: 1px solid var(--border); border-radius: 12px; padding: 20px; width: 100%; max-width: 400px; box-shadow: 0 8px 30px rgba(0,0,0,.25); }
    .modal-title { font-size: 16px; font-weight: 600; margin: 0 0 8px; }
    .modal-message { font-size: 14px; color: var(--text-secondary); margin: 0 0 20px; line-height: 1.6; }
    .modal-actions { display: flex; justify-content: flex-end; gap: 8px; }

    /* ========== 空状态 ========== */
    .empty { display: flex; flex-direction: column; align-items: center; gap: 6px; padding: 36px 20px; text-align: center; color: var(--text-secondary); }
    .empty-icon { color: var(--text-tertiary); }
    .empty-title { font-size: 14px; color: var(--text); font-weight: 500; }
    .empty-hint { font-size: 12.5px; }

    /* ========== 登录页 ========== */
    .login-wrap { min-height: 100vh; display: flex; align-items: center; justify-content: center; padding: 16px; }
    .login-card { background: var(--card); border: 1px solid var(--border); border-radius: 14px; padding: 36px 32px; width: 100%; max-width: 380px; box-shadow: var(--shadow-card); }
    .login-logo { display: flex; justify-content: center; color: var(--accent); }
    .login-title { font-size: 22px; font-weight: 700; color: var(--text); text-align: center; margin: 12px 0 0; }
    .login-sub { font-size: 13px; color: var(--text-secondary); text-align: center; margin: 6px 0 24px; }
    .login-card .btn { width: 100%; justify-content: center; margin-top: 16px; }

    /* ========== 概览页摘要 ========== */
    .summary-row { display: flex; justify-content: space-between; align-items: center; gap: 12px; padding: 8px 0; border-bottom: 1px solid var(--border); font-size: 13.5px; }
    .summary-row:last-child { border-bottom: none; }
    .error-line { display: flex; gap: 8px; align-items: flex-start; font-size: 13px; padding: 4px 0; }
    .warning-box { background: var(--warning-soft); border: 1px solid color-mix(in srgb, var(--warning) 35%, transparent); border-radius: 8px; padding: 10px 14px; }

    /* ========== 配置页 ========== */
    .link-row { display: flex; align-items: center; gap: 12px; padding: 10px 0; border-bottom: 1px solid var(--border); }
    .link-row:last-child { border-bottom: none; }
    .link-label { font-weight: 500; font-size: 13.5px; flex-shrink: 0; width: 76px; }
    .link-url { flex: 1; min-width: 0; font-family: var(--font-mono); font-size: 12.5px; color: var(--text-secondary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .token-row { display: flex; align-items: center; gap: 10px; padding: 10px 0; }
    .token-row + .token-row { border-top: 1px solid var(--border); }
    .token-label { font-size: 13.5px; color: var(--text-secondary); width: 84px; flex-shrink: 0; }
    .token-value { flex: 1; min-width: 0; font-family: var(--font-mono); font-size: 13px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

    /* ========== 图标动画 ========== */
    .spinner { animation: spin .8s linear infinite; }
    @keyframes spin { to { transform: rotate(360deg); } }

    /* ========== 响应式：窄屏收成顶栏 ========== */
    @media (max-width: 768px) {
      .app-shell { flex-direction: column; }
      .sidebar { width: 100%; height: auto; position: sticky; top: 0; z-index: 10; flex-direction: row; align-items: center; padding: 10px 12px; border-right: none; border-bottom: 1px solid var(--border); }
      .sidebar-brand { padding: 0; margin-right: auto; }
      .nav { flex-direction: row; gap: 4px; }
      .nav-item { width: auto; padding: 8px 10px; }
      .nav-item span { display: none; }
      .sidebar-footer { border-top: none; padding: 0; flex-direction: row; margin-left: 8px; }
      .sidebar-version { display: none; }
      .main { padding: 16px; }
      .page-head { flex-direction: column; align-items: flex-start; }
      .stats-grid { grid-template-columns: repeat(2, 1fr); }
      .grid-2 { grid-template-columns: 1fr; }
      .form-row { flex-direction: column; align-items: stretch; }
      .form-row .btn { width: 100%; justify-content: center; }
    }
    @media (max-width: 480px) { .stats-grid { grid-template-columns: 1fr; } }
  </style>
</head>
<body>
  <!-- dioxus 0.8 的 launch 默认挂载点（dx build 时注入 WASM 入口脚本） -->
  <div id="main"></div>
</body>
</html>
```

- [ ] **Step 2: 构建验证**

```bash
cd crates/server/web && dx build --web --release
```
Expected: exit 0。样式此时不影响编译（HTML 内联），此步骤验证无语法破坏。

- [ ] **Step 3: Commit**

```bash
git add crates/server/web/index.html
git commit -m "style(web): add dual-theme design system CSS

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 2: SVG 图标组件（icon.rs）

**Files:**
- Create: `crates/server/web/src/components/icon.rs`

**Interfaces:**
- Consumes: 无（纯 dioxus）
- Produces: `pub fn icon(name: &'static str, size: u32) -> Element`；`#[component] pub fn Spinner(size: u32) -> Element`

- [ ] **Step 1: 创建 icon.rs**

```rust
// crates/server/web/src/components/icon.rs
// 内联 SVG 图标（Lucide 风格：stroke 线稿，fill=none）。
// 不依赖任何图标库，全部手写 path；颜色跟随 currentColor（由父元素 color 控制）。
use dioxus::prelude::*;

pub fn icon(name: &'static str, size: u32) -> Element {
    match name {
        // logo：嵌套六边形（工具/聚合感）
        "logo" => rsx! {
            svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
                path { d: "M12 2 20 6.5v11L12 22 4 17.5v-11L12 2Z" }
                path { d: "M12 6.5 16.5 9v6L12 17.5 7.5 15V9l4.5-2.5Z" }
            }
        },
        // overview：仪表盘
        "overview" => rsx! {
            svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
                path { d: "m12 14 4-4" }
                path { d: "M3.34 19a10 10 0 1 1 17.32 0" }
            }
        },
        // sources：链节
        "sources" => rsx! {
            svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
                path { d: "M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71" }
                path { d: "M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71" }
            }
        },
        // preview：眼睛
        "preview" => rsx! {
            svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
                path { d: "M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0" }
                circle { cx: "12", cy: "12", r: "3" }
            }
        },
        // config：齿轮
        "config" => rsx! {
            svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
                path { d: "M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" }
                circle { cx: "12", cy: "12", r: "3" }
            }
        },
        // logout：出门箭头
        "logout" => rsx! {
            svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
                path { d: "M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4" }
                polyline { points: "16 17 21 12 16 7" }
                line { x1: "21", x2: "9", y1: "12", y2: "12" }
            }
        },
        // refresh：环形箭头
        "refresh" => rsx! {
            svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
                path { d: "M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8" }
                path { d: "M21 3v5h-5" }
                path { d: "M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16" }
                path { d: "M8 16H3v5" }
            }
        },
        // copy：双矩形
        "copy" => rsx! {
            svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
                rect { width: "14", height: "14", x: "8", y: "8", rx: "2", ry: "2" }
                path { d: "M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2" }
            }
        },
        // trash：垃圾桶
        "trash" => rsx! {
            svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
                path { d: "M3 6h18" }
                path { d: "M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6" }
                path { d: "M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2" }
            }
        },
        // plus：加号
        "plus" => rsx! {
            svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
                path { d: "M5 12h14" }
                path { d: "M12 5v14" }
            }
        },
        // check：对勾
        "check" => rsx! {
            svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
                path { d: "M20 6 9 17l-5-5" }
            }
        },
        // x：关闭
        "x" => rsx! {
            svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
                path { d: "M18 6 6 18" }
                path { d: "m6 6 12 12" }
            }
        },
        // alert：三角警告
        "alert" => rsx! {
            svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", stroke_linejoin: "round",
                path { d: "m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3" }
                path { d: "M12 9v4" }
                path { d: "M12 17h.01" }
            }
        },
        _ => rsx! { span {} },
    }
}

// 旋转加载图标：CSS .spinner 动画驱动。
#[component]
pub fn Spinner(size: u32) -> Element {
    rsx! {
        svg { width: size, height: size, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", stroke_linecap: "round", class: "spinner",
            path { d: "M21 12a9 9 0 1 1-6.219-8.56" }
        }
    }
}
```

- [ ] **Step 2: 在 mod.rs 注册模块并构建**

`crates/server/web/src/components/mod.rs` 追加一行（当前内容为 4 个 `pub mod`，保持风格）：

```rust
pub mod icon;
```

```bash
cd crates/server/web && dx build --web --release
```
Expected: exit 0。若 svg 属性名（view_box/stroke_width 等）报错，检查是否用了下划线形式（dioxus rsx 对 svg 元素自动转 snake_case 属性）。

- [ ] **Step 3: Commit**

```bash
git add crates/server/web/src/components/icon.rs crates/server/web/src/components/mod.rs
git commit -m "feat(web): add inline SVG icon component

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 3: Toast 系统（toast.rs）

**Files:**
- Create: `crates/server/web/src/components/toast.rs`

**Interfaces:**
- Consumes: `icon::icon`（"x"、"alert"、"check" 图标）
- Produces: `ToastKind`、`ToastMsg`、`ToastProvider`、`use_toast`、`push_toast`、`schedule_timeout`（签名见 File Structure）

- [ ] **Step 1: 创建 toast.rs**

```rust
// crates/server/web/src/components/toast.rs
// 全局 Toast：ToastProvider 提供 Signal<Vec<ToastMsg>> context，渲染右上角堆叠。
// 每条 4s 自动消失；use_effect 在 dioxus 0.8 alpha 无依赖数组（每次渲染都跑），
// 用 scheduled 信号保证定时器只注册一次。
use crate::components::icon::icon;
use dioxus::prelude::*;
use wasm_bindgen::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ToastKind {
    Success,
    Error,
    Info,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToastMsg {
    pub id: u64,
    pub kind: ToastKind,
    pub text: String,
}

thread_local! {
    static NEXT_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
}

/// 追加一条 toast（自动分配自增 id）。
pub fn push_toast(mut toasts: Signal<Vec<ToastMsg>>, kind: ToastKind, text: impl Into<String>) {
    let id = NEXT_ID.with(|c| {
        let v = c.get();
        c.set(v + 1);
        v
    });
    toasts.write().push(ToastMsg { id, kind, text: text.into() });
}

/// 在 ToastProvider 子树内读取 toast 信号。
pub fn use_toast() -> Signal<Vec<ToastMsg>> {
    use_context::<Signal<Vec<ToastMsg>>>()
}

/// wasm setTimeout 封装：ms 毫秒后执行 f（Closure 生命周期由 JS 定时器持有，forget 是有意的）。
pub(crate) fn schedule_timeout(ms: u32, f: impl FnOnce() + 'static) {
    let cb = Closure::once(f);
    if let Some(w) = web_sys::window() {
        let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(cb.as_ref().unchecked_ref(), ms);
    }
    cb.forget();
}

#[component]
pub fn ToastProvider() -> Element {
    let toasts = use_context_provider(|| Signal::new(Vec::<ToastMsg>::new()));
    let items = toasts.read().clone();
    rsx! {
        div { class: "toast-stack",
            for t in items {
                ToastCard { toasts, t }
            }
        }
    }
}

#[component]
fn ToastCard(toasts: Signal<Vec<ToastMsg>>, t: ToastMsg) -> Element {
    let id = t.id;
    let kind = t.kind;
    let text = t.text.clone();
    let mut scheduled = use_signal(|| false);
    use_effect(move || {
        if scheduled() {
            return;
        }
        scheduled.set(true);
        let mut toasts = toasts.clone();
        schedule_timeout(4000, move || {
            toasts.write().retain(|x| x.id != id);
        });
    });

    let icon_name = match kind {
        ToastKind::Success => "check",
        ToastKind::Error => "alert",
        ToastKind::Info => "config",
    };
    let kind_class = match kind {
        ToastKind::Success => "success",
        ToastKind::Error => "error",
        ToastKind::Info => "info",
    };
    let mut toasts = toasts.clone();
    rsx! {
        div { class: format!("toast {kind_class}"),
            {icon(icon_name, 15)}
            span { "{text}" }
            button { class: "toast-close", onclick: move |_| { toasts.write().retain(|x| x.id != id); }, {icon("x", 13)} }
        }
    }
}
```

注意：`ToastKind::Info` 图标用 "config"（齿轮）——想换成感叹号时只需改这里，不新增图标名。

- [ ] **Step 2: 注册模块并构建**

`crates/server/web/src/components/mod.rs` 追加：

```rust
pub mod toast;
```

```bash
cd crates/server/web && dx build --web --release
```
Expected: exit 0。

- [ ] **Step 3: Commit**

```bash
git add crates/server/web/src/components/toast.rs crates/server/web/src/components/mod.rs
git commit -m "feat(web): add global toast system

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 4: 确认弹窗（confirm.rs）

**Files:**
- Create: `crates/server/web/src/components/confirm.rs`

**Interfaces:**
- Consumes: 无
- Produces: `ConfirmState`、`ConfirmDialog(state: Signal<ConfirmState>, on_confirm: EventHandler<()>)`

- [ ] **Step 1: 创建 confirm.rs**

```rust
// crates/server/web/src/components/confirm.rs
// 通用确认弹窗：页面持有 ConfirmState 信号（open=false 时不渲染任何内容），
// 确认/取消/遮罩点击都会关闭；danger=true 时确认按钮为红色。
use dioxus::prelude::*;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ConfirmState {
    pub open: bool,
    pub title: String,
    pub message: String,
    pub confirm_text: String,
    pub danger: bool,
}

#[component]
pub fn ConfirmDialog(state: Signal<ConfirmState>, on_confirm: EventHandler<()>) -> Element {
    if !state.read().open {
        return None;
    }
    let title = state.read().title.clone();
    let message = state.read().message.clone();
    let confirm_text = state.read().confirm_text.clone();
    let danger = state.read().danger;
    let mut state = state.clone();
    let on_confirm = on_confirm.clone();
    rsx! {
        div { class: "modal-overlay", onclick: move |_| state.set(ConfirmState::default()),
            div { class: "modal", onclick: move |e| e.stop_propagation(),
                h3 { class: "modal-title", "{title}" }
                p { class: "modal-message", "{message}" }
                div { class: "modal-actions",
                    button { class: "btn btn-ghost", onclick: move |_| state.set(ConfirmState::default()), "取消" }
                    button { class: if danger { "btn btn-danger" } else { "btn btn-primary" },
                        onclick: move |_| on_confirm.call(()),
                        "{confirm_text}"
                    }
                }
            }
        }
    }
}
```

- [ ] **Step 2: 注册模块并构建**

`crates/server/web/src/components/mod.rs` 追加：

```rust
pub mod confirm;
```

```bash
cd crates/server/web && dx build --web --release
```
Expected: exit 0。

- [ ] **Step 3: Commit**

```bash
git add crates/server/web/src/components/confirm.rs crates/server/web/src/components/mod.rs
git commit -m "feat(web): add confirm dialog component

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 5: 登录页重设计（login.rs）

**Files:**
- Rewrite: `crates/server/web/src/components/login.rs`

**Interfaces:**
- Consumes: `icon::icon`（"logo"）、`icon::Spinner`
- Produces: `read_token/write_token/clear_token`（签名不变）、`Login(on_login: EventHandler<String>)`

- [ ] **Step 1: 重写 login.rs**

保留 `read_token/write_token/clear_token` 三个函数原样不动，只重写 `Login` 组件：

```rust
// crates/server/web/src/components/login.rs
use crate::api::request;
use crate::components::icon::{icon, Spinner};
use dioxus::prelude::*;

pub fn read_token() -> Option<String> {
    let w = web_sys::window()?;
    let s = w.local_storage().ok().flatten()?;
    s.get_item("submerge_admin_token").ok().flatten()
}

pub fn write_token(t: &str) {
    if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = s.set_item("submerge_admin_token", t);
    }
}

pub fn clear_token() {
    if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = s.remove_item("submerge_admin_token");
    }
}

#[component]
pub fn Login(on_login: EventHandler<String>) -> Element {
    let mut input = use_signal(String::new);
    let mut error = use_signal(String::new);
    let mut loading = use_signal(|| false);

    // 无参闭包：onclick（MouseEvent）与 onkeydown（KeyboardEvent）两种入口共享同一流程。
    // 不能直接把一个闭包同时传给两种事件处理器——Rust 闭包参数类型固定，两种事件类型不兼容。
    let do_submit = move || {
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
        div { class: "login-wrap",
            div { class: "login-card",
                div { class: "login-logo", {icon("logo", 40)} }
                div { class: "login-title", "sub-merge" }
                p { class: "login-sub", "订阅聚合与转换管理" }
                div { class: "field",
                    input {
                        type: "password",
                        placeholder: "管理 token",
                        value: input,
                        oninput: move |e| input.set(e.value()),
                        onkeydown: move |e| {
                            if e.key() == Key::Enter {
                                do_submit();
                            }
                        },
                    }
                }
                if !error.read().is_empty() {
                    p { class: "error-text", "{error}" }
                }
                button { class: "btn btn-primary", onclick: move |_| do_submit(), disabled: *loading.read(),
                    if *loading.read() {
                        rsx! { Spinner { size: 14 } }
                    } else {
                        rsx! { "登录" }
                    }
                }
            }
        }
    }
}
```

- [ ] **Step 2: 构建**

```bash
cd crates/server/web && dx build --web --release
```
Expected: exit 0。

- [ ] **Step 3: Commit**

```bash
git add crates/server/web/src/components/login.rs
git commit -m "feat(web): redesign login page

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 6: 概览页（overview.rs）+ sources.rs 共享函数

**Files:**
- Create: `crates/server/web/src/components/overview.rs`
- Modify: `crates/server/web/src/components/sources.rs`（仅追加 `fetch_sources`，不动组件）

**Interfaces:**
- Consumes: `sources::SourceDto`、`sources::fetch_sources`、`icon`、`Spinner`
- Produces: `Overview(token, on_goto: EventHandler<usize>)`

- [ ] **Step 1: 在 sources.rs 追加 fetch_sources**

在 `SourceDto` 定义之后、`Sources` 组件之前插入：

```rust
pub async fn fetch_sources(token: Option<&str>) -> Result<Vec<SourceDto>, String> {
    let body = request("GET", "/api/admin/sources", None, token).await?;
    serde_json::from_str(&body).map_err(|e| format!("解析失败: {}", e))
}
```

- [ ] **Step 2: 创建 overview.rs**

```rust
// crates/server/web/src/components/overview.rs
// 概览页：4 张统计卡片（源总数/启用中/节点总数/失败源数）+ 订阅源摘要 + 最近错误。
// 数据来自现有两个接口（sources + preview），纯客户端聚合。
use crate::api::request;
use crate::components::icon::{icon, Spinner};
use crate::components::sources::{fetch_sources, SourceDto};
use dioxus::prelude::*;
use serde::Deserialize;

// 只取需要的字段；serde 默认忽略未知字段（nodes 等）。
#[derive(Debug, Clone, Deserialize)]
struct PreviewSummary {
    total: usize,
    errors: Vec<String>,
}

async fn fetch_preview(token: Option<&str>) -> Result<PreviewSummary, String> {
    let body = request("GET", "/api/admin/preview", None, token).await?;
    serde_json::from_str(&body).map_err(|e| format!("解析失败: {}", e))
}

#[component]
pub fn Overview(token: Signal<Option<String>>, on_goto: EventHandler<usize>) -> Element {
    let sources = use_signal(Vec::<SourceDto>::new);
    let stats = use_signal(|| None::<PreviewSummary>);
    let error = use_signal(String::new);
    let loading = use_signal(|| false);

    // 初次挂载加载一次。
    use_future(move || {
        let token = token.read().clone();
        let mut sources = sources;
        let mut stats = stats;
        let mut error = error;
        let mut loading = loading;
        async move {
            loading.set(true);
            error.set(String::new());
            match fetch_sources(token.as_deref()).await {
                Ok(list) => sources.set(list),
                Err(e) => error.set(e),
            }
            match fetch_preview(token.as_deref()).await {
                Ok(s) => stats.set(Some(s)),
                Err(e) => error.set(e),
            }
            loading.set(false);
        }
    });

    let reload = move |_| {
        let token = token.read().clone();
        let mut sources = sources.clone();
        let mut stats = stats.clone();
        let mut error = error.clone();
        let mut loading = loading.clone();
        spawn(async move {
            loading.set(true);
            error.set(String::new());
            match fetch_sources(token.as_deref()).await {
                Ok(list) => sources.set(list),
                Err(e) => error.set(e),
            }
            match fetch_preview(token.as_deref()).await {
                Ok(s) => stats.set(Some(s)),
                Err(e) => error.set(e),
            }
            loading.set(false);
        });
    };

    // 统计值在 rsx 外预计算（避免借用冲突）。
    let source_total = sources.read().len();
    let enabled_count = sources.read().iter().filter(|s| s.enabled).count();
    let (node_total, failed_count) = stats
        .read()
        .as_ref()
        .map(|s| (s.total, s.errors.len()))
        .unwrap_or((0, 0));

    let source_rows: Vec<Element> = sources
        .read()
        .iter()
        .map(|s| {
            let enabled = s.enabled;
            let name = s.name.clone();
            rsx! {
                div { class: "summary-row",
                    span { "{name}" }
                    span { class: format!("badge {}", if enabled { "on" } else { "off" }),
                        if enabled { "启用" } else { "停用" }
                    }
                }
            }
        })
        .collect();

    let error_rows: Vec<Element> = stats
        .read()
        .as_ref()
        .map(|s| {
            s.errors
                .iter()
                .map(|e| {
                    let e = e.clone();
                    rsx! {
                        div { class: "error-line", {icon("alert", 14)} span { "{e}" } }
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let on_goto = on_goto.clone();
    rsx! {
        div { class: "page-head",
            h1 { class: "page-title", "概览" }
            button { class: "btn btn-secondary", onclick: reload, disabled: *loading.read(),
                if *loading.read() {
                    rsx! { Spinner { size: 14 } }
                } else {
                    rsx! { {icon("refresh", 14)} }
                }
                "刷新"
            }
        }
        if !error.read().is_empty() {
            p { class: "error-text", "{error}" }
        }
        div { class: "stats-grid",
            StatCard { icon_name: "sources", value: source_total.to_string(), label: "订阅源总数", danger: false }
            StatCard { icon_name: "check", value: enabled_count.to_string(), label: "启用中", danger: false }
            StatCard { icon_name: "preview", value: node_total.to_string(), label: "节点总数", danger: false }
            StatCard { icon_name: "alert", value: failed_count.to_string(), label: "失败源数", danger: failed_count > 0 }
        }
        div { class: "grid-2",
            div { class: "card",
                h2 { class: "card-title", "订阅源" }
                if source_rows.is_empty() {
                    div { class: "empty",
                        {icon("sources", 36)}
                        span { class: "empty-title", "暂无订阅源" }
                        span { class: "empty-hint", "前往「订阅源」页面添加第一个源" }
                    }
                } else {
                    {source_rows.into_iter()}
                    div { class: "card-foot",
                        button { class: "btn btn-ghost btn-sm", onclick: move |_| on_goto.call(1), "管理订阅源" }
                    }
                }
            }
            div { class: "card",
                h2 { class: "card-title", "最近错误" }
                if error_rows.is_empty() {
                    div { class: "empty",
                        {icon("check", 36)}
                        span { class: "empty-title", "全部正常" }
                        span { class: "empty-hint", "最近一次合并没有失败源" }
                    }
                } else {
                    div { class: "warning-box", {error_rows.into_iter()} }
                }
            }
        }
    }
}

#[component]
fn StatCard(icon_name: &'static str, value: String, label: &'static str, danger: bool) -> Element {
    rsx! {
        div { class: "stat-card",
            div { class: if danger { "stat-icon danger" } else { "stat-icon" },
                {icon(icon_name, 18)}
            }
            div {
                div { class: "stat-value", "{value}" }
                div { class: "stat-label", "{label}" }
            }
        }
    }
}
```

- [ ] **Step 3: 注册模块并构建**

`crates/server/web/src/components/mod.rs` 追加：

```rust
pub mod overview;
```

```bash
cd crates/server/web && dx build --web --release
```
Expected: exit 0。

- [ ] **Step 4: Commit**

```bash
git add crates/server/web/src/components/overview.rs crates/server/web/src/components/sources.rs crates/server/web/src/components/mod.rs
git commit -m "feat(web): add overview dashboard page

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 7: 主壳侧边栏导航（main.rs）

**Files:**
- Rewrite: `crates/server/web/src/main.rs`

**Interfaces:**
- Consumes: `Login`、`Overview`、`Sources`、`Preview`、`Config`、`ToastProvider`、`icon`、`clear_token/read_token/write_token`
- Produces: 新 App/MainShell 结构（MainShell 内部定义 NavItem 子组件）

- [ ] **Step 1: 重写 main.rs**

```rust
// crates/server/web/src/main.rs
mod api;
mod components;

use components::config::Config;
use components::icon::icon;
use components::login::{clear_token, read_token, write_token, Login};
use components::overview::Overview;
use components::preview::Preview;
use components::sources::Sources;
use components::toast::ToastProvider;
use dioxus::prelude::*;

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    let mut token = use_signal(|| read_token());
    rsx! {
        match token().as_deref() {
            Some(_) => rsx! {
                // ToastProvider 必须包在需要 toast 的子树外层（context 只向下传播）。
                ToastProvider {
                    MainShell { token }
                }
            },
            None => rsx! {
                Login {
                    on_login: move |t: String| {
                        write_token(&t);
                        token.set(Some(t));
                    },
                }
            },
        }
    }
}

// 主壳：侧边栏导航（窄屏自动收成顶栏，见 CSS @media 768px）。
#[component]
fn MainShell(token: Signal<Option<String>>) -> Element {
    let mut tab = use_signal(|| 0usize);
    // use_callback 让 on_goto 句柄跨渲染稳定（避免 EventHandler::new 在 render 体累积）。
    let on_goto = use_callback(move |t: usize| tab.set(t));
    rsx! {
        div { class: "app-shell",
            aside { class: "sidebar",
                div { class: "sidebar-brand",
                    {icon("logo", 22)}
                    span { "sub-merge" }
                }
                nav { class: "nav",
                    NavItem { name: "overview", label: "概览", active: *tab.read() == 0, onnav: move |_| tab.set(0) }
                    NavItem { name: "sources", label: "订阅源", active: *tab.read() == 1, onnav: move |_| tab.set(1) }
                    NavItem { name: "preview", label: "预览", active: *tab.read() == 2, onnav: move |_| tab.set(2) }
                    NavItem { name: "config", label: "配置", active: *tab.read() == 3, onnav: move |_| tab.set(3) }
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
                    match *tab.read() {
                        0 => rsx! { Overview { token, on_goto } },
                        1 => rsx! { Sources { token } },
                        2 => rsx! { Preview { token } },
                        _ => rsx! { Config { token } },
                    }
                }
            }
        }
    }
}

#[component]
fn NavItem(name: &'static str, label: &'static str, active: bool, onnav: EventHandler<MouseEvent>) -> Element {
    rsx! {
        button { class: if active { "nav-item active" } else { "nav-item" }, onclick: onnav,
            {icon(name, 16)}
            span { "{label}" }
        }
    }
}
```

- [ ] **Step 2: 构建**

```bash
cd crates/server/web && dx build --web --release
```
Expected: exit 0。

- [ ] **Step 3: Commit**

```bash
git add crates/server/web/src/main.rs
git commit -m "feat(web): sidebar navigation shell

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 8: 订阅源页重设计（sources.rs）

**Files:**
- Rewrite: `crates/server/web/src/components/sources.rs`（保留 Step 6.1 加的 `SourceDto` + `fetch_sources`）

**Interfaces:**
- Consumes: `request`、`fetch_sources`、`SourceDto`、`ConfirmDialog/ConfirmState`、`icon/Spinner`、`toast::{use_toast, push_toast, ToastKind}`
- Produces: 无新接口（SourceDto/fetch_sources 签名不变）

- [ ] **Step 1: 重写 sources.rs 组件**

保留 `SourceDto` 与 `fetch_sources`（Task 6 已加）原样，`Sources` 组件整体替换为：

```rust
#[component]
pub fn Sources(token: Signal<Option<String>>) -> Element {
    let sources = use_signal(Vec::<SourceDto>::new);
    let mut error = use_signal(String::new);
    let mut new_url = use_signal(String::new);
    let mut new_name = use_signal(String::new);
    let mut adding = use_signal(|| false);
    let mut refreshing = use_signal(std::collections::HashSet::<i64>::new);
    let mut confirm = use_signal(ConfirmState::default);
    let mut pending_id = use_signal(|| None::<i64>);
    let toasts = use_toast();

    // 初次挂载加载一次。
    use_future(move || {
        let token = token.read().clone();
        let mut sources = sources;
        let mut error = error;
        async move {
            match fetch_sources(token.as_deref()).await {
                Ok(list) => sources.set(list),
                Err(e) => error.set(e),
            }
        }
    });

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
        let mut new_url = new_url.clone();
        let mut new_name = new_name.clone();
        let mut error = error.clone();
        let mut adding = adding.clone();
        let mut toasts = toasts.clone();
        adding.set(true);
        spawn(async move {
            match request("POST", "/api/admin/sources", Some(body), token.as_deref()).await {
                Ok(_) => {
                    match fetch_sources(token.as_deref()).await {
                        Ok(list) => sources.set(list),
                        Err(e) => error.set(e),
                    }
                    new_url.set(String::new());
                    new_name.set(String::new());
                    error.set(String::new());
                    push_toast(toasts, ToastKind::Success, "订阅源已添加");
                }
                Err(e) => error.set(format!("添加失败: {e}")),
            }
            adding.set(false);
        });
    };

    let toggle = move |id: i64, enabled: bool| {
        let token = token.read().clone();
        let body = serde_json::json!({ "enabled": !enabled }).to_string();
        let mut sources = sources.clone();
        let mut error = error.clone();
        let mut toasts = toasts.clone();
        spawn(async move {
            match request("PUT", &format!("/api/admin/sources/{id}"), Some(body), token.as_deref()).await {
                Ok(_) => {
                    match fetch_sources(token.as_deref()).await {
                        Ok(list) => sources.set(list),
                        Err(e) => error.set(e),
                    }
                    push_toast(toasts, ToastKind::Info, if enabled { "已停用" } else { "已启用" });
                }
                Err(e) => push_toast(toasts, ToastKind::Error, format!("操作失败: {e}")),
            }
        });
    };

    let refresh = move |id: i64| {
        if refreshing.read().contains(&id) {
            return;
        }
        refreshing.write().insert(id);
        let token = token.read().clone();
        let mut refreshing = refreshing.clone();
        let mut toasts = toasts.clone();
        spawn(async move {
            match request("POST", &format!("/api/admin/sources/{id}/refresh"), None, token.as_deref()).await {
                Ok(body) => match serde_json::from_str::<serde_json::Value>(&body) {
                    Ok(v) => {
                        let name = v.get("source").and_then(|s| s.as_str()).unwrap_or("该源");
                        match v.get("ok").and_then(|o| o.as_bool()) {
                            Some(true) => {
                                let n = v.get("node_count").and_then(|c| c.as_u64()).unwrap_or(0);
                                push_toast(toasts, ToastKind::Success, format!("{} 已刷新：{} 个节点", name, n));
                            }
                            _ => {
                                let reason = v.get("reason").and_then(|r| r.as_str()).unwrap_or("未知错误");
                                push_toast(toasts, ToastKind::Error, format!("{} 刷新失败：{}", name, reason));
                            }
                        }
                    }
                    Err(e) => push_toast(toasts, ToastKind::Error, format!("刷新失败: {}", e)),
                },
                Err(e) => push_toast(toasts, ToastKind::Error, format!("刷新失败: {e}")),
            }
            refreshing.write().remove(&id);
        });
    };

    let ask_delete = move |id: i64| {
        let name = sources
            .read()
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.name.clone())
            .unwrap_or_default();
        pending_id.set(Some(id));
        confirm.set(ConfirmState {
            open: true,
            title: "删除订阅源".into(),
            message: format!("确定删除「{}」？此操作不可撤销。", name),
            confirm_text: "删除".into(),
            danger: true,
        });
    };

    // 确认删除：关闭弹窗 → 执行 DELETE → 重新加载列表。
    let on_confirm_delete = use_callback(move |_: ()| {
        confirm.set(ConfirmState::default());
        if let Some(id) = pending_id() {
            let token = token.read().clone();
            let mut sources = sources.clone();
            let mut error = error.clone();
            let mut toasts = toasts.clone();
            spawn(async move {
                match request("DELETE", &format!("/api/admin/sources/{id}"), None, token.as_deref()).await {
                    Ok(_) => {
                        match fetch_sources(token.as_deref()).await {
                            Ok(list) => sources.set(list),
                            Err(e) => error.set(e),
                        }
                        push_toast(toasts, ToastKind::Success, "已删除");
                    }
                    Err(e) => push_toast(toasts, ToastKind::Error, format!("删除失败: {e}")),
                }
            });
        }
    });

    // 行预渲染成 owned Element（沿用项目既有模式，避免 E0716 借用问题）。
    let rows: Vec<Element> = sources
        .read()
        .iter()
        .map(|s| {
            let id = s.id;
            let enabled = s.enabled;
            let name = s.name.clone();
            let url = s.url.clone();
            let busy = refreshing.read().contains(&id);
            rsx! {
                tr {
                    td { class: "cell-name", "{name}" }
                    td { class: "cell-url", title: "{url}", "{url}" }
                    td {
                        span { class: format!("badge {}", if enabled { "on" } else { "off" }),
                            if enabled { "启用" } else { "停用" }
                        }
                    }
                    td {
                        div { class: "actions",
                            button { class: "btn btn-ghost btn-sm", onclick: move |_| toggle(id, enabled), disabled: busy,
                                {icon(if enabled { "x" } else { "check" }, 13)}
                                if enabled { "停用" } else { "启用" }
                            }
                            button { class: "btn btn-ghost btn-sm", onclick: move |_| refresh(id), disabled: busy,
                                if busy {
                                    rsx! { Spinner { size: 12 } }
                                } else {
                                    rsx! { {icon("refresh", 13)} }
                                }
                                "刷新"
                            }
                            button { class: "btn btn-danger btn-sm", onclick: move |_| ask_delete(id),
                                {icon("trash", 13)}
                                "删除"
                            }
                        }
                    }
                }
            }
        })
        .collect();

    let mut error_for_render = error.clone();
    rsx! {
        div { class: "page-head",
            h1 { class: "page-title", "订阅源" }
        }
        if !error_for_render.read().is_empty() {
            p { class: "error-text", "{error_for_render}" }
        }
        div { class: "card",
            h2 { class: "card-title", "添加订阅源" }
            div { class: "form-row",
                div { class: "field",
                    label { "订阅 URL" }
                    input {
                        class: "mono",
                        placeholder: "https://example.com/sub",
                        value: new_url,
                        oninput: move |e| new_url.set(e.value()),
                    }
                }
                div { class: "field",
                    label { "名称" }
                    input {
                        placeholder: "例如：机场 A",
                        value: new_name,
                        oninput: move |e| new_name.set(e.value()),
                    }
                }
                button { class: "btn btn-primary", onclick: add, disabled: *adding.read(),
                    if *adding.read() {
                        rsx! { Spinner { size: 14 } }
                    } else {
                        rsx! { {icon("plus", 14)} }
                    }
                    "添加"
                }
            }
        }
        div { class: "card",
            h2 { class: "card-title", "订阅源列表" }
            if rows.is_empty() {
                div { class: "empty",
                    {icon("sources", 36)}
                    span { class: "empty-title", "暂无订阅源" }
                    span { class: "empty-hint", "在上方表单填写名称与订阅 URL，点击「添加」开始" }
                }
            } else {
                div { class: "table-wrap",
                    table {
                        thead {
                            tr { th { "名称" } th { "URL" } th { "状态" } th { "操作" } }
                        }
                        tbody {
                            {rows.into_iter()}
                        }
                    }
                }
            }
        }
        ConfirmDialog { state: confirm, on_confirm: on_confirm_delete }
    }
}
```

- [ ] **Step 2: 构建**

```bash
cd crates/server/web && dx build --web --release
```
Expected: exit 0。

- [ ] **Step 3: Commit**

```bash
git add crates/server/web/src/components/sources.rs
git commit -m "feat(web): redesign sources page

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 9: 预览页重设计（preview.rs）

**Files:**
- Rewrite: `crates/server/web/src/components/preview.rs`

**Interfaces:**
- Consumes: `request`、`icon/Spinner`
- Produces: 无新接口（内部 `PreviewResp/PreviewNode` 仍为模块私有）

- [ ] **Step 1: 重写 preview.rs**

```rust
// crates/server/web/src/components/preview.rs
// 转换预览：节点表（协议彩色徽章）+ 源错误警告卡片。
use crate::api::request;
use crate::components::icon::{icon, Spinner};
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

// 协议 → 配色（CSS --proto-0..5）。同族协议同色。
fn proto_class(protocol: &str) -> &'static str {
    match protocol {
        "ss" | "ssr" => "proto-0",
        "vmess" | "vless" => "proto-1",
        "trojan" => "proto-2",
        "hysteria" | "hysteria2" => "proto-3",
        "tuic" => "proto-4",
        _ => "proto-5",
    }
}

#[component]
pub fn Preview(token: Signal<Option<String>>) -> Element {
    let data = use_signal(|| None::<PreviewResp>);
    let loading = use_signal(|| false);
    let error = use_signal(String::new);

    // 初次挂载加载一次。
    use_future(move || {
        let token = token.read().clone();
        let mut data = data;
        let mut loading = loading;
        let mut error = error;
        async move {
            loading.set(true);
            error.set(String::new());
            match request("GET", "/api/admin/preview", None, token.as_deref()).await {
                Ok(body) => match serde_json::from_str::<PreviewResp>(&body) {
                    Ok(r) => data.set(Some(r)),
                    Err(e) => error.set(format!("解析失败: {}", e)),
                },
                Err(e) => error.set(e),
            }
            loading.set(false);
        }
    });

    let reload = move |_| {
        let token = token.read().clone();
        let mut data = data.clone();
        let mut loading = loading.clone();
        let mut error = error.clone();
        spawn(async move {
            loading.set(true);
            error.set(String::new());
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

    let resp = data.read().clone();
    let rows: Vec<Element> = resp
        .as_ref()
        .map(|r| {
            r.nodes
                .iter()
                .map(|n| {
                    let name = n.name.clone();
                    let protocol = n.protocol.clone();
                    let server = n.server.clone();
                    let port = n.port;
                    rsx! {
                        tr {
                            td { class: "cell-name", "{name}" }
                            td { span { class: format!("proto {}", proto_class(&protocol)), "{protocol}" } }
                            td { class: "cell-url", "{server}" }
                            td { "{port}" }
                        }
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let error_rows: Vec<Element> = resp
        .as_ref()
        .map(|r| {
            r.errors
                .iter()
                .map(|e| {
                    let e = e.clone();
                    rsx! {
                        div { class: "error-line", {icon("alert", 14)} span { "{e}" } }
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    rsx! {
        div { class: "page-head",
            h1 { class: "page-title", "转换预览" }
            if let Some(r) = resp.as_ref() {
                span { class: "badge on", "共 {r.total} 个节点" }
            }
            button { class: "btn btn-secondary", onclick: reload, disabled: *loading.read(),
                if *loading.read() {
                    rsx! { Spinner { size: 14 } }
                } else {
                    rsx! { {icon("refresh", 14)} }
                }
                "刷新预览"
            }
        }
        if !error.read().is_empty() {
            p { class: "error-text", "{error}" }
        }
        if let Some(r) = resp.as_ref() {
            if r.nodes.is_empty() {
                div { class: "empty",
                    {icon("preview", 36)}
                    span { class: "empty-title", "暂无节点" }
                    span { class: "empty-hint", "检查订阅源是否已启用、刷新后重试" }
                }
            } else {
                div { class: "table-wrap",
                    table {
                        thead {
                            tr { th { "名称" } th { "协议" } th { "服务器" } th { "端口" } }
                        }
                        tbody {
                            {rows.into_iter()}
                        }
                    }
                }
            }
            if !r.errors.is_empty() {
                h2 { class: "card-title", style: "margin-top: 20px", "源错误" }
                div { class: "warning-box", {error_rows.into_iter()} }
            }
        }
    }
}
```

- [ ] **Step 2: 构建**

```bash
cd crates/server/web && dx build --web --release
```
Expected: exit 0。

- [ ] **Step 3: Commit**

```bash
git add crates/server/web/src/components/preview.rs
git commit -m "feat(web): redesign preview page

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 10: 配置页重设计（config.rs）

**Files:**
- Rewrite: `crates/server/web/src/components/config.rs`

**Interfaces:**
- Consumes: `request`、`write_token`、`ConfirmDialog/ConfirmState`、`icon/Spinner`、`toast::{use_toast, push_toast, ToastKind, schedule_timeout}`
- Produces: 无新接口

- [ ] **Step 1: 重写 config.rs**

```rust
// crates/server/web/src/components/config.rs
// 配置页：订阅链接卡片（复制反馈）+ Token 管理（掩码显示 + 轮换确认）。
use crate::api::request;
use crate::components::confirm::{ConfirmDialog, ConfirmState};
use crate::components::icon::icon;
use crate::components::login::write_token;
use crate::components::toast::{push_toast, schedule_timeout, use_toast, ToastKind};
use dioxus::prelude::*;
use serde::Deserialize;
use std::rc::Rc;
use wasm_bindgen_futures::JsFuture;

#[derive(Debug, Clone, Deserialize)]
pub struct ConfigDto {
    pub subscribe_token: String,
    pub admin_token: String,
    pub subscribe_url: String,
}

// web-sys 0.3.103 实测签名（与计划里的说明不同）：
//   Window::navigator() -> Navigator（直接返回，非 Option）
//   Navigator::clipboard() -> Clipboard（直接返回，非 Result）
//   Clipboard::write_text(&str) -> js_sys::Promise（用 JsFuture await）
//   Window::location() -> Location；Location::href() -> Result<String, JsValue>
fn copy_text(text: String) {
    if let Some(nav) = web_sys::window().map(|w| w.navigator()) {
        let clip = nav.clipboard();
        spawn(async move {
            let _ = JsFuture::from(clip.write_text(&text)).await;
        });
    }
}

#[component]
pub fn Config(token: Signal<Option<String>>) -> Element {
    let cfg = use_signal(|| None::<ConfigDto>);
    let error = use_signal(String::new);
    let mut copied = use_signal(|| None::<&'static str>);
    let mut show_admin = use_signal(|| false);
    let mut confirm = use_signal(ConfirmState::default);
    let mut pending_rotate = use_signal(|| None::<&'static str>);
    let toasts = use_toast();

    // 初次挂载加载一次。
    use_future(move || {
        let token = token.read().clone();
        let mut cfg = cfg.clone();
        let mut error = error.clone();
        async move {
            match request("GET", "/api/admin/config", None, token.as_deref()).await {
                Ok(body) => match serde_json::from_str::<ConfigDto>(&body) {
                    Ok(c) => cfg.set(Some(c)),
                    Err(e) => error.set(format!("解析失败: {}", e)),
                },
                Err(e) => error.set(e),
            }
        }
    });

    let rotate = move |which: &'static str| {
        let current = token.read().clone();
        let body = serde_json::json!({ "rotate": which }).to_string();
        let mut cfg = cfg.clone();
        let mut error = error.clone();
        let mut toasts = toasts.clone();
        // Signal 是 Copy：把 token signal 拷进闭包，rotating admin token 后同步会话。
        let mut token = token;
        spawn(async move {
            match request("PUT", "/api/admin/config", Some(body), current.as_deref()).await {
                Ok(b) => match serde_json::from_str::<ConfigDto>(&b) {
                    Ok(c) => {
                        // 服务端轮换 admin token 后，旧 token 立即失效（已实测 401）。
                        // 同步更新本地会话（localStorage + token signal）。
                        if which == "admin" {
                            write_token(&c.admin_token);
                            token.set(Some(c.admin_token.clone()));
                        }
                        error.set(String::new());
                        cfg.set(Some(c));
                        push_toast(toasts, ToastKind::Success, format!("{} token 已轮换", if which == "admin" { "管理" } else { "订阅" }));
                    }
                    Err(e) => error.set(format!("解析失败: {}", e)),
                },
                Err(e) => error.set(e),
            }
        });
    };

    let ask_rotate = move |which: &'static str| {
        pending_rotate.set(Some(which));
        let admin = which == "admin";
        confirm.set(ConfirmState {
            open: true,
            title: format!("轮换{} token", if admin { "管理" } else { "订阅" }),
            message: if admin {
                "轮换后旧管理 token 立即失效，当前会话将自动更新为新 token。确定继续？".into()
            } else {
                "轮换后旧订阅 token 立即失效，所有已复制的订阅链接需要重新复制。确定继续？".into()
            },
            confirm_text: "轮换".into(),
            danger: admin,
        });
    };

    let on_confirm_rotate = use_callback(move |_: ()| {
        confirm.set(ConfirmState::default());
        if let Some(which) = pending_rotate() {
            rotate(which);
        }
    });

    let copy_click = move |label: &'static str, link: String| {
        copy_text(link);
        copied.set(Some(label));
        let mut copied = copied.clone();
        schedule_timeout(2000, move || {
            copied.set(None);
        });
    };

    // base_url 取当前页面 origin（协议 + 主机 + 端口，不含路径）。
    // 不能用 href()：页面 URL 带路径（如 /index.html）时会把路径拼进订阅链接。
    let base_url = web_sys::window()
        .and_then(|w| w.location().origin().ok())
        .unwrap_or_default();

    // 订阅链接在 rsx 外预计算（rsx 内嵌 format! 的 {} 会被误判为插值）。
    let links: Vec<(&'static str, String)> = cfg
        .read()
        .as_ref()
        .map(|c| {
            [("Clash", "clash"), ("V2Ray", "v2ray"), ("Sing-box", "singbox")]
                .into_iter()
                .map(|(label, fmt)| {
                    let link = format!("{}{}?token={}&format={}", base_url, c.subscribe_url, c.subscribe_token, fmt);
                    (label, link)
                })
                .collect()
        })
        .unwrap_or_default();

    let link_rows: Vec<Element> = links
        .iter()
        .map(|(label, link)| {
            let label = *label;
            // 事件处理器是 FnMut，会多次调用；用 Rc 在闭包内 clone，避免 move 出 captured String。
            let link_for_copy = Rc::new(link.clone());
            let is_copied = *copied.read() == Some(label);
            rsx! {
                div { class: "link-row",
                    span { class: "link-label", "{label}" }
                    code { class: "link-url", "{link}" }
                    button {
                        class: format!("btn btn-ghost btn-sm{}", if is_copied { " checked" } else { "" }),
                        onclick: move |_| {
                            copy_click(label, link_for_copy.as_ref().clone());
                        },
                        {icon("copy", 13)}
                        if is_copied { "已复制" } else { "复制" }
                    }
                }
            }
        })
        .collect();

    let mut cfg_render = cfg.clone();
    let mut show_admin_render = show_admin.clone();
    rsx! {
        div { class: "page-head",
            h1 { class: "page-title", "配置" }
        }
        if let Some(c) = cfg_render.read().as_ref() {
            div { class: "card",
                h2 { class: "card-title", "订阅链接" }
                p { class: "subtle", "将以下链接填入 Clash / V2Ray / Sing-box 客户端的订阅地址" }
                {link_rows.into_iter()}
            }
            div { class: "card",
                h2 { class: "card-title", "Token" }
                p { class: "subtle", "管理 token 轮换后，当前浏览器会话自动切换到新 token；其他设备需重新登录。" }
                div { class: "token-row",
                    span { class: "token-label", "订阅 token" }
                    code { class: "token-value", "{c.subscribe_token}" }
                    button { class: "btn btn-secondary btn-sm", onclick: move |_| ask_rotate("subscribe"), "轮换" }
                }
                div { class: "token-row",
                    span { class: "token-label", "管理 token" }
                    code { class: "token-value",
                        "{if *show_admin_render.read() { c.admin_token.clone() } else { "••••••••".to_string() }}"
                    }
                    button { class: "btn btn-ghost btn-sm",
                        onclick: move |_| {
                            let v = *show_admin_render.read();
                            show_admin_render.set(!v);
                        },
                        if *show_admin_render.read() { "隐藏" } else { "显示" }
                    }
                    button { class: "btn btn-danger btn-sm", onclick: move |_| ask_rotate("admin"), "轮换" }
                }
            }
        }
        if !error.read().is_empty() {
            p { class: "error-text", "{error}" }
        }
        ConfirmDialog { state: confirm, on_confirm: on_confirm_rotate }
    }
}
```

- [ ] **Step 2: 构建**

```bash
cd crates/server/web && dx build --web --release
```
Expected: exit 0。

- [ ] **Step 3: Commit**

```bash
git add crates/server/web/src/components/config.rs
git commit -m "feat(web): redesign config page

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 11: 全量验证

**Files:**
- 无代码改动（仅验证）

- [ ] **Step 1: 构建 + 冒烟**

```bash
cd crates/server/web && dx build --web --release
make smoke
```
Expected: smoke 全部通过（SPA 静态资源 + API）。

- [ ] **Step 2: 启动并人工验证**

```bash
make run
```
浏览器打开 `http://localhost:8080`，按清单逐项核对（对照规格 §8）：

1. **登录页**：居中卡片、logo、密码框（圆点）、Enter 可提交、错误 token 显示红色错误、正确 token 进入
2. **侧边栏**：桌面端左侧栏（logo + 4 项 + 版本号 + 退出登录）；当前页高亮
3. **概览页**：4 张统计卡片数值正确（与源列表/预览一致）、失败源卡片红色、空状态文案
4. **订阅源页**：添加表单（URL/名称）添加成功 Toast + 列表刷新；启停用 Toast；刷新按钮 spinner + 节点数 Toast；删除弹确认框（取消不删、确认删除 + Toast）；无源时空状态
5. **预览页**：协议徽章配色、共 N 个节点徽章、失败源 warning 卡片、空数据空状态
6. **配置页**：三个链接卡片复制按钮（点击变「已复制」2s 后还原）、token 掩码（•••••••• 显示/隐藏）、轮换订阅 token（确认框 → Toast）、轮换管理 token（确认框 → Toast → 会话保持登录）
7. **深色主题**：系统切换深色模式后页面各组件（侧边栏/卡片/表格/徽章/弹窗/toast）配色正常，无白底残留
8. **响应式**：窗口 <768px 侧边栏收成顶栏（图标导航）、表格可横向滚动、统计卡片 2 列 → 1 列
9. **回归**：退出登录回到登录页；重新登录后数据仍正常

发现问题 → 记入下一任务前的修复提交（`fix(web): ...`）；全部通过则本计划完成。

- [ ] **Step 3: 收尾提交（如有修复）**

```bash
git add -A && git commit -m "fix(web): verification fixes

Co-Authored-By: Claude <noreply@anthropic.com>"
```
（无修复则跳过本步。）
