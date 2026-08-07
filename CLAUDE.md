# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目

sub-merge：订阅链接聚合与转换工具。聚合多个订阅源，实时并发拉取并合并为一个订阅，统一输出 Clash YAML / V2Ray base64 / Sing-box JSON 三种格式。小圈子自用，token 鉴权。

## 架构

- `crates/proxy-core`：纯逻辑。11 种协议解析（ss/ssr/socks5/http/vmess/vless/trojan/hysteria/hysteria2/tuic/wireguard）、3 种格式序列化（clash/v2ray/singbox）。协议/格式的单元测试与 roundtrip/proptest 测试在此。
- `crates/server`：axum 服务。SQLite（sqlx）持久化 sources/settings/combined_subs/combined_sources。路由：`/subscribe/{name}`（组合订阅输出，按组合成员拉取，无鉴权）、`/admin/*`（源 CRUD/预览/配置/组合订阅 CRUD，Bearer 鉴权）；`/healthz`；SPA 静态托管 + 前端路由回退。
- `crates/server/web`（submerge-web）：**独立于 workspace**（不加入 members，避免 dx build 与 cargo build --workspace 冲突），只能由 dx 构建。dioxus 0.8.0-alpha.1（精确版本锁定）+ WASM。样式全部在 `index.html` 内联 CSS（CSS 变量双主题，`prefers-color-scheme` 自动切换）。
- `crates/web-core`（submerge-web-core）：前端纯逻辑库（API 契约 DTO、ApiError、映射/格式化函数），**在 workspace 内**，由 `cargo test --workspace` 覆盖；无 dioxus/web-sys 依赖，host 原生可测。web crate 以 path 依赖引用。

## 常用命令

```bash
make build-web     # 前端 WASM：cd crates/server/web && dx build --web --release --debug-symbols false
make build-server  # cargo build --release -p server
make run           # 构建前端并启动服务（默认 :8080）
make smoke         # 端到端冒烟：构建 → 临时 server → curl 验证 SPA/静态/API

cargo test --workspace                  # 后端与纯逻辑测试（web-core 在 workspace 内；web crate 不在，仅由 dx 构建）
cd crates/server/web && dx build --web --release --debug-symbols false   # 前端唯一构建方式
```

## 强制要求

1. **每次代码修改后，必须按顺序全部通过：**

   ```bash
   cargo upgrade -i
   cargo fmt --all
   cargo clippy --workspace
   cargo test --workspace
   ```

2. **代码一律使用 Rust edition 2024**（根 workspace 已设 `edition = "2024"`，crates 经 `edition.workspace = true` 继承，web crate 为字面 `edition = "2024"`）。新增 crate 保持 2024，不得引入旧 edition。

## dioxus 0.8 alpha 已验证的坑（web crate 开发必读）

- rsx 的 if/else 分支内**不能嵌套 `rsx!` 宏调用**，须直接写元素形式：`if cond { Spinner { size: 14 } } else { "文案" }`；`match` 分支嵌套 `rsx!` 实测可编译（main.rs 的 App/MainShell 在用），但属文档禁区——重构为 if/else 会直接破坏构建，新增分支一律写元素形式
- `Element` 是 `Result<VNode, RenderError>`，组件空渲染用 `VNode::empty()`（不是 `return None`）
- `use_effect` 无依赖数组（每次渲染都跑），需要一次性逻辑时用信号做守卫
- `EventHandler<T> = Callback<T>`；跨渲染稳定的回调用 `use_callback` 创建，不要直接在 render 体 `EventHandler::new`
- `Signal` 可直接调用取值（`signal()`）；`Signal::set/write` 使闭包变 FnMut，绑定处可能需要 `let mut`
- svg 属性在 rsx 里用 snake_case（`view_box`、`stroke_width`）；`web-sys` 的 `setTimeout` 接受 `i32`（需 `as i32`）
- 前端 UI 无测试 harness，验证 = `dx build` + `make smoke` + 浏览器人工核对
- 前端**纯逻辑**（web-core）在 workspace 内，由 `cargo test --workspace` 覆盖；测试设计 spec 见 `docs/specs/2026-08-07-web-core-testing-design.md`，实施计划见 `docs/plans/2026-08-07-web-core-testing.md`
- release 构建必须带 `--debug-symbols false`：dx 0.8.0-alpha.1 的 `debug_symbols` CLI 默认 true，且在 build/web.rs 里**无条件覆盖** dx.toml 的 `[web.wasm_opt] debug` 键（配置键无效，仅 CLI 标志生效）。不带该标志时 dx 给 wasm-opt 传 `--debuginfo`，binaryen 127（dx 固定版本）解析 rustc 1.97 的新 DWARF 报 `compile unit size was incorrect` 并 SIGABRT——dx 会打印 ERROR 但仍继续完成构建（非致命），产物带 DWARF 且更大
- **web-sys 非 Result 导入的 JS 调用会炸整页**：`Clipboard::write_text` 这类返回裸 `Promise` 的导入，底层 JS 抛异常（属性不存在/被拒）时 web-sys 不会转成 `Result`，异常穿透成 Rust panic，wasm32 panic=abort 直接整页失效（无任何报错提示）。实测：`navigator.clipboard` 在非安全上下文（http://局域网IP 访问）为 `undefined`，点击复制按钮即卡死。**调用可能 throw 的 JS API 前必须探测**——`copy_text` 用 `js_sys::Reflect::get(nav.as_ref(), "clipboard")` 守卫后返回 Err 走 toast

## 环境变量与 token（详见 README.md）

| 变量 | 默认值 | 说明 |
|------|--------|------|
| PORT | 8080 | 监听端口 |
| DATABASE_PATH | ./submerge.db | SQLite 路径 |
| CONCURRENCY | 8 | 并发拉取上限 |
| TIMEOUT_SECS | 15 | 单源超时 |
| MAX_NODES | 2000 | 节点总数上限 |
| WEB_DIST | ./web/dist | 前端静态资源目录（git-ignored symlink → dx 构建产物） |
| SUB_MERGE_ADMIN_TOKEN | 随机生成 | 预设初始 admin token（仅首次初始化时生效，已部署实例不受影响） |

首次启动日志打印一次随机 token（重启不重复）；部署时用上述 `SUB_MERGE_ADMIN_TOKEN` 预设。管理接口一律 `Authorization: Bearer <admin_token>`。
