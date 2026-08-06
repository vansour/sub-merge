# sub-merge 完善设计（硬化与输入支持）

- 日期：2026-08-06
- 状态：已批准
- 前置：2026-08-06 全项目代码审查（5 视角 + 27 条候选问题置信度验证）

## 1. 背景与目标

代码审查确认 10 条问题（≥80 置信度）+ 16 条边缘问题（70-79）。经与用户澄清，本项目核心需求是**合并订阅源节点、输出 Clash/V2Ray/Sing-box 三种格式**，不做逐协议参数保真。

### 目标
- 修复使**输出结构无效**的问题（非法 URI、无法加载的 YAML、整订阅 500）
- 补齐合并体验的关键缺口（Clash YAML 订阅输入、超大输入防护）
- 工程化完善（README、CI、测试增强）

### 非目标（已确认）
- 模型字段级保真扩展（username、vless flow/pbk/sid、SSR protocol/obfs、wireguard IP/MTU、hy2 obfs、tuic 参数等一律不新增字段）
- shadowtls 实现（改为**移除死代码** `ShadowTlsConfig` 并更新原设计文档）
- socks5/http 伪造用户名修复（保持现状）
- SS 2022-blake3-aes-256-gcm → plain 的 cipher 修复（保持现状）
- v2ray 格式扩展 5 类协议（保持跳过，README 文档注明）
- 部分源失败在订阅响应中标记（保持现状，preview API 已展示错误）
- 全局并发信号量（保持每请求信号量）
- 原规范中的新功能（去重/过滤/测速/缓存）

## 2. 架构与数据流

无架构变化。仍为 proxy-core（纯逻辑）+ axum 服务 + Dioxus 前端三层。数据流不变（实时拉取 → 解析 → 合并 → 序列化）。

本次改动集中在 proxy-core 的序列化/解析层与 server 的静态/抓取层。

## 3. 阶段 1：输出有效性修复（proxy-core）

### 3.1 hysteria2 序列化补 `?` 前缀

**文件**：`crates/proxy-core/src/protocols/hysteria2.rs`（serialize_hysteria2，现 62-88 行）

现状：`?sni=` 仅在 sni 存在时写出；`&alpn=`、`&insecure=` 无条件追加。sni=None 且有 alpn/insecure 时产出 `hysteria2://pass@host:8443&alpn=h3`——缺 `?`，本项目自己的解析器以 InvalidPort 拒绝（95 置信度，实证）。

修复：查询参数条件组装——`sni`/`alpn`/`insecure` 任一存在时先写 `?`，后续参数以 `&` 连接。

### 3.2 trojan 序列化补 `?` 前缀

**文件**：`crates/proxy-core/src/protocols/trojan.rs`（serialize_trojan，现 119-135 行）

现状：`?security=tls` 仅在 tls.enabled 时写出；`&type=`、`&host=`、`&path=` 无条件追加。无 TLS 且有传输层时产出无 `?` 的非法 URI（75 置信度，实证）。

修复：同 3.1 的条件组装。

### 3.3 trojan 解析默认启用 TLS

**文件**：`crates/proxy-core/src/protocols/trojan.rs`（parse_trojan，现 57-67 行）

现状：`security` 参数仅为 tls/reality/xtls 时启用 TLS。标准分享格式 `trojan://pass@host:443#name`（无 query）解析为 tls: None，singbox 输出无 TLS 的 trojan outbound，无法连接（75 置信度）。

修复：trojan 协议强制 TLS 承载——无 `security` 参数时 `tls.enabled = true`（与 `parse_clash_yaml` 中 trojan 始终 TLS 的语义一致，parser.rs:123-127）。sni 等字段仍按现有逻辑（sni 缺省时用 host）。

副作用（可接受）：序列化输出自动带 `?security=tls`；往返幂等。

### 3.4 Clash YAML 标量转义兜底

**文件**：`crates/proxy-core/src/formats/clash.rs`（clash_yaml_str，现 28-34 行）

现状：仅对空白/`:`/`#`/`,`/`[`/`]` 加引号。节点名/密码来自不可信订阅文本，含 YAML flow indicator（`!`、`*`、`&`、`{`、`}`、`|`、`>`、`%`、`` ` `` 等开头或包含）时产出无法加载的配置（75 置信度，PyYAML 实证：tag/alias/flow-mapping 解析错误、块标量吞行）。

修复：`clash_yaml_str` 改为——值仅含 ASCII 字母数字与 `._-` 四种字符时原样输出；否则用 `serde_yaml::to_string` 序列化该字符串值（自动处理引号、反斜杠转义、flow indicator），保证任何输入都产出合法 YAML 标量。

## 4. 阶段 2：输入支持与健壮性

### 4.1 Clash YAML 订阅自动接入

**文件**：`crates/proxy-core/src/parser.rs`（parse_subscription_text，现 65-76 行）

现状：`parse_clash_yaml` 完整实现（79-161 行）但零生产调用；唯一入口 `parse_subscription_text` 只做 base64/逐行 URI 解析，Clash YAML 源确定性得到 0 节点（90 置信度）。

修复：`parse_subscription_text` 增加 YAML 检测——任一行的 trim 结果以 `proxies:` 开头时走 `parse_clash_yaml`（成功返回其节点，失败回退逐行解析）。检测在 base64 尝试之前执行（YAML 文本不会被误当 base64）。

`service.rs`（fetch_and_merge）与 `routes/sources.rs`（refresh_source）均走 `parse_subscription_text`，无需改动。

### 4.2 超大输入截断

**文件**：`crates/proxy-core/src/parser.rs`、`crates/proxy-core/src/uri.rs`、`crates/server/src/service.rs`

规范 §5 承诺"恶意超大行截断（>1MB）"但从未实现（85 置信度）。

修复：
- `parse_lines`：单行 >1MB 跳过并计入 skipped（不解析）
- `decode_base64_url`：输入长度 >4MB 返回 `ParseError::InvalidBase64`（解码膨胀防护）
- `fetch_source`：读取 body 后若 >16MB 截断（reqwest `resp.bytes()` 已整读，截断防后续解析爆炸）

### 4.3 wireguard 单节点失败降级

**文件**：`crates/proxy-core/src/formats/clash.rs`（serialize_clash，现 5-26 行）

现状：`proxy_to_clash(n)?` 遇单个序列化失败（如 wireguard 缺 privateKey）传播错误，整个 `/api/subscribe`（默认 clash 格式）返回 500（75 置信度）；singbox 用 filter_map 静默跳过，行为不一致。

修复：`serialize_clash` 改为逐节点容错（filter 失败节点），与 singbox 行为统一。wireguard 节点缺 privateKey 时被跳过而非拖垮整订阅。

### 4.4 `/api` 防护补丁

**文件**：`crates/server/src/static.rs`（fallback，现 12 行）

现状：`uri.path().starts_with("/api/")` 仅覆盖带尾斜杠形式；`/api`（无斜杠）、`//api/x`（双斜杠）绕过防护落入 SPA 回退，返回 HTML 200 而非统一 JSON 404（70 置信度）。

修复：改为 `uri.path().trim_start_matches('/').starts_with("api")`，覆盖上述形态。无鉴权/数据暴露风险，纯响应一致性。

## 5. 阶段 3：工程化

### 5.1 README.md（新建）

内容：项目简介与架构、快速开始（make run）、Docker 部署（多阶段构建说明）、环境变量表（PORT/DATABASE_PATH/CONCURRENCY/TIMEOUT_SECS/MAX_NODES/WEB_DIST）、API 文档（订阅接口 + 管理接口 + 鉴权说明）、token 获取方式、**v2ray 格式节点覆盖说明**（socks5/http/hy1/hy2/wireguard 节点在此格式被跳过）、测试与冒烟说明。

### 5.2 CI（新建 `.github/workflows/ci.yml`）

- job `check`：`cargo fmt --check` + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo test --workspace`（缓存 ~/.cargo）
- job `web`：`cargo install dioxus-cli --version 0.8.0-alpha.1`（缓存 cargo 安装）+ `dx build --web` + 验证 dist 产出
- job `docker`：`docker build -t sub-merge .`（验证多阶段构建）
- 注：仓库当前无远端，workflow 在 push 到远端后生效

### 5.3 测试增强

| 位置 | 用例 |
|------|------|
| `tests/roundtrip.rs` | hy2：alpn 有值、sni 为 None 的往返；trojan：有传输层、无 TLS 的往返 |
| `tests/parser_dispatch.rs` | Clash YAML 文本经 `parse_subscription_text` 自动识别 |
| `tests/formats.rs` | 恶意名称（`!secret`、`*alias`、`|` 开头）生成配置用 serde_yaml 解析验证 |
| `crates/server/tests/api_test.rs` | wireguard 坏节点 → 200 且跳过该节点；`/api` 与 `//api/x` → JSON 404；超长行（>1MB）被跳过 |
| `parser.rs`/`uri.rs` 内嵌测试 | 单行 1MB 截断、base64 输入 4MB 上限 |

## 6. 错误处理

- 无新错误类型。
- `serialize_clash` 从"整体失败"改为"逐节点跳过"（3 节描述），`serialize_nodes` 签名不变。
- 截断行为：跳过并计数（parse 层），不报错。

## 7. 兼容性

- DB schema、API 路由、请求/响应格式：无变化。
- `ProxyNode` 模型：无变化。
- 行为变化（有意）：trojan 无 security 参数时视为 TLS；clash 序列化单节点失败跳过；`/api` 变体返回 JSON 404；Clash YAML 源可解析；超长输入被截断。

## 8. 提交策略

按阶段提交（每阶段 `cargo test --workspace` 全绿）：

1. `fix(proxy-core): valid query-string assembly for hysteria2/trojan, trojan TLS default`
2. `fix(proxy-core): safe clash yaml scalar quoting, per-node tolerant serialization`
3. `feat(proxy-core): clash yaml subscription auto-detection, oversized input truncation`
4. `fix(server): /api guard covers path-shape variants`
5. `docs: README, drop shadowtls dead code from model and design doc`
6. `ci: github actions workflow`
7. `test: regression coverage for hardening changes`

涉及文件：proxy-core 6 个（hysteria2/trojan/clash/parser/uri/model）、server 2 个（static.rs、service.rs）、新增 2 个（README.md、.github/workflows/ci.yml）、测试 5 个。
