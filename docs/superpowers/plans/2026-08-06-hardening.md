# sub-merge 硬化实施计划（输出有效性 / 输入支持 / 工程化）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复输出结构无效问题（hy2/trojan 非法 URI、trojan 无 TLS、Clash YAML 引号）、接入 Clash YAML 订阅输入与超大输入截断、wireguard 单节点失败降级、`/api` 防护补丁，并完成 README/CI/测试增强。

**Architecture:** 改动集中在 proxy-core 的序列化/解析层（5 个文件）与 server 的静态/抓取层（2 个文件）。`ProxyNode` 模型除移除 shadowtls 死代码外**不改字段**。数据流（实时拉取 → 解析 → 合并 → 序列化）不变。

**Tech Stack:** Rust 1.97 (edition 2024)、serde_yaml、reqwest、axum、GitHub Actions

## Global Constraints

- workspace 根：`/root/github/sub-merge`；`cargo test --workspace` 必须全绿
- `ProxyNode` 模型不新增字段（保留 username/flow/SSR 参数等现状）
- 输出格式（clash/v2ray/singbox）结构保持兼容，行为有意变更仅限：trojan 无 security 默认 TLS、clash 序列化单节点失败跳过、`/api` 变体 JSON 404、Clash YAML 源可解析、超长输入截断
- 不修复：socks5/http 用户名、SS 2022 cipher、v2ray 格式节点覆盖、部分源失败标记、全局信号量
- 每个 Task 以 TDD 完成：先写失败测试 → 实现 → 全绿 → commit
- 提交信息用仓库现有约定前缀（fix/feat/docs/ci/test）
- 依赖版本不变，不新增第三方依赖

---

### Task 1: hysteria2 序列化 `?` 前缀修复

**Files:**
- Modify: `crates/proxy-core/src/protocols/hysteria2.rs:62-88`（serialize_hysteria2）
- Test: `crates/proxy-core/tests/protocol_hysteria2.rs`

**Interfaces:**
- Consumes: `serialize_hysteria2(node: &ProxyNode) -> Result<String, SerializeError>`（现有签名不变）
- Produces: 无新接口；修复后任何输入都产出可被 `parse_hysteria2` 再解析的 URI

- [ ] **Step 1: 写失败测试**

在 `crates/proxy-core/tests/protocol_hysteria2.rs` 追加：

```rust
#[test]
fn hysteria2_serialize_without_sni_emits_valid_query() {
    // alpn/insecure 存在但 sni 为 None 时，query 必须以 ? 开头（回归：缺 ? 产出非法 URI）
    let n = parse_hysteria2("hysteria2://pass@1.2.3.4:8443?alpn=h3&insecure=1#T").unwrap();
    assert!(n.tls.as_ref().unwrap().sni.is_none());
    let out = serialize_hysteria2(&n).unwrap();
    assert!(out.contains("?alpn=h3"), "query must start with '?': {out}");
    assert!(out.contains("&insecure=1"), "params joined with &: {out}");
    // 输出必须能被自己解析
    let n2 = parse_hysteria2(&out).unwrap();
    assert_eq!(n2.server, "1.2.3.4");
    assert_eq!(n2.port, 8443);
    assert_eq!(n2.tls.as_ref().unwrap().alpn, vec!["h3".to_string()]);
    assert!(n2.tls.as_ref().unwrap().insecure);
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p proxy-core --test protocol_hysteria2`
Expected: FAIL — `out` 为 `hysteria2://pass@1.2.3.4:8443&alpn=h3&insecure=1#T`，`contains("?alpn=h3")` 断言失败

- [ ] **Step 3: 实现修复**

`serialize_hysteria2` 的 TLS 段改为条件组装：

```rust
    // 查询参数条件组装：任一参数存在时先写 '?'，参数间用 '&' 连接
    let mut query: Vec<String> = Vec::new();
    if let Some(t) = &node.tls {
        if let Some(s) = &t.sni {
            query.push(format!("sni={}", encode(s)));
        }
        if let Some(a) = t.alpn.first() {
            query.push(format!("alpn={}", encode(a)));
        }
        if t.insecure {
            query.push("insecure=1".into());
        }
    }
    if !query.is_empty() {
        out.push('?');
        out.push_str(&query.join("&"));
    }
```

即把原 `if let Some(s) = &t.sni { out.push_str(&format!("?sni={}", ...)) }` / `&alpn=` / `&insecure=` 三处替换为上面代码。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p proxy-core --test protocol_hysteria2`
Expected: PASS（5 个测试，含新测试）

- [ ] **Step 5: Commit**

```bash
git add crates/proxy-core/src/protocols/hysteria2.rs crates/proxy-core/tests/protocol_hysteria2.rs
git commit -m "fix(proxy-core): hysteria2 serializer emits valid '?'-prefixed query"
```

---

### Task 2: trojan 序列化 `?` 前缀修复

**Files:**
- Modify: `crates/proxy-core/src/protocols/trojan.rs:119-141`（serialize_trojan）
- Test: `crates/proxy-core/tests/protocol_trojan.rs`

**Interfaces:**
- Consumes: `serialize_trojan(node: &ProxyNode) -> Result<String, SerializeError>`（现有签名不变）
- Produces: 无新接口；修复后无 TLS 但有传输层的 trojan 节点产出可解析 URI

- [ ] **Step 1: 写失败测试**

在 `crates/proxy-core/tests/protocol_trojan.rs` 追加：

```rust
#[test]
fn trojan_serialize_transport_without_tls_emits_valid_query() {
    // security=none（显式关闭 TLS）+ ws 传输：query 必须以 ? 开头（回归：缺 ? 产出非法 URI）
    let n = parse_trojan(
        "trojan://pass@1.2.3.4:443?security=none&type=ws&path=%2Fws&host=cdn.example.com#T",
    )
    .unwrap();
    assert!(n.tls.is_none());
    assert!(n.transport.as_ref().and_then(|t| t.websocket.as_ref()).is_some());
    let out = serialize_trojan(&n).unwrap();
    assert!(out.contains("?type=ws"), "query must start with '?': {out}");
    assert!(out.contains("&host=cdn.example.com"), "params joined with &: {out}");
    let n2 = parse_trojan(&out).unwrap();
    assert_eq!(n2.port, 443);
    let ws = n2.transport.as_ref().and_then(|t| t.websocket.as_ref()).unwrap();
    assert_eq!(ws.path, "/ws");
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p proxy-core --test protocol_trojan`
Expected: FAIL — `out` 为 `trojan://pass@1.2.3.4:443&type=ws&host=cdn.example.com&path=%2Fws`，断言失败

- [ ] **Step 3: 实现修复**

`serialize_trojan` 中从 `if let Some(t) = &node.tls {` 到 `path` 拼接段整体替换为条件组装：

```rust
    let mut query: Vec<String> = Vec::new();
    if let Some(t) = &node.tls {
        if t.enabled {
            query.push("security=tls".into());
        }
        if let Some(s) = &t.sni {
            query.push(format!("sni={}", encode(s)));
        }
        if let Some(fp) = &t.fingerprint {
            query.push(format!("fp={}", encode(fp)));
        }
        if !t.alpn.is_empty() {
            query.push(format!("alpn={}", encode(&t.alpn.join(","))));
        }
    }
    if net != "tcp" {
        query.push(format!("type={}", net));
    }
    if !host.is_empty() {
        query.push(format!("host={}", encode(&host)));
    }
    if !path.is_empty() {
        query.push(format!("path={}", encode(&path)));
    }
    if !query.is_empty() {
        out.push('?');
        out.push_str(&query.join("&"));
    }
```

（原代码为：`if let Some(t) = &node.tls { if t.enabled { out.push_str("?security=tls"); } ... }` 后跟无条件 `&type=`/`&host=`/`&path=` 拼接。）

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p proxy-core --test protocol_trojan`
Expected: PASS（5 个测试，含新测试）

- [ ] **Step 5: Commit**

```bash
git add crates/proxy-core/src/protocols/trojan.rs crates/proxy-core/tests/protocol_trojan.rs
git commit -m "fix(proxy-core): trojan serializer emits valid '?'-prefixed query"
```

---

### Task 3: trojan 解析默认启用 TLS

**Files:**
- Modify: `crates/proxy-core/src/protocols/trojan.rs:57-67`（parse_trojan 的 TLS 判断）
- Test: `crates/proxy-core/tests/protocol_trojan.rs`

**Interfaces:**
- Consumes: `parse_trojan(uri: &str) -> Result<ProxyNode, ParseError>`（现有签名不变）
- Produces: 无新接口；行为变更——trojan 协议强制 TLS，仅当 `security=none` 显式出现时 tls 为 None

- [ ] **Step 1: 写失败测试**

在 `crates/proxy-core/tests/protocol_trojan.rs` 追加：

```rust
#[test]
fn trojan_defaults_to_tls_without_security_param() {
    // 标准分享格式无 security 参数：trojan 协议强制 TLS 承载
    let n = parse_trojan("trojan://pass@1.2.3.4:443#T").unwrap();
    let tls = n.tls.expect("trojan without security param must default to TLS");
    assert!(tls.enabled);
    // 显式 security=none 仍然关闭 TLS（保持序列化修复的测试语义）
    let n2 = parse_trojan("trojan://pass@1.2.3.4:443?security=none#T").unwrap();
    assert!(n2.tls.is_none());
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p proxy-core --test protocol_trojan`
Expected: FAIL — `n.tls.expect(...)` panic（无 security 参数时 tls 为 None）

- [ ] **Step 3: 实现修复**

`parse_trojan` 中：

```rust
    let tls = if security == "none" {
        None
    } else {
        Some(TlsSettings {
            enabled: true,
            sni: sni.or(host.clone()),
            alpn,
            insecure,
            fingerprint: fp,
        })
    };
```

（原代码为 `if matches!(security.as_str(), "tls" | "reality" | "xtls") { Some(...) } else { None }`。与 `parse_clash_yaml` 中 trojan 始终 TLS 的语义对齐。）

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p proxy-core --test protocol_trojan` 然后 `cargo test -p proxy-core`
Expected: 全部 PASS（含 roundtrip 中 trojan 用例——输入均带 security=tls，行为不变）

- [ ] **Step 5: Commit**

```bash
git add crates/proxy-core/src/protocols/trojan.rs crates/proxy-core/tests/protocol_trojan.rs
git commit -m "fix(proxy-core): trojan defaults to TLS when security param absent"
```

---

### Task 4: Clash YAML 标量转义兜底（serde_yaml）

**Files:**
- Modify: `crates/proxy-core/src/formats/clash.rs:28-34`（clash_yaml_str）
- Test: `crates/proxy-core/tests/formats.rs`

**Interfaces:**
- Consumes: `clash_yaml_str(s: &str) -> String`（内部函数，签名不变）
- Produces: 无新接口；任何节点名/密码都产出合法 YAML 标量

- [ ] **Step 1: 写失败测试**

在 `crates/proxy-core/tests/formats.rs` 追加：

```rust
#[test]
fn clash_yaml_quotes_hostile_names() {
    use proxy_core::model::{Crypto, Protocol, ProxyNode};
    for name in ["!secret", "*alias", "a|b", "col:on", "a\"b", "{x}", "p@ss", "日本 东京"] {
        let node = ProxyNode {
            name: name.into(),
            kind: Protocol::Ss,
            server: "1.2.3.4".into(),
            port: 8388,
            crypto: Some(Crypto::Aes256Gcm),
            password: Some("pw".into()),
            ..Default::default()
        };
        let out = serialize_clash(&[node]).unwrap();
        let v: serde_yaml::Value = serde_yaml::from_str(&out)
            .unwrap_or_else(|e| panic!("output must be valid yaml for {name:?}: {e}\n{out}"));
        assert_eq!(
            v["proxies"][0]["name"].as_str().unwrap(),
            name,
            "name must roundtrip for {name:?}"
        );
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p proxy-core --test formats`
Expected: FAIL — `!secret` / `*alias` 等产出非法 YAML（serde_yaml::from_str 报 tag/alias 错误），或 name 不相等

- [ ] **Step 3: 实现修复**

`clash_yaml_str` 替换为：

```rust
fn clash_yaml_str(s: &str) -> String {
    // 仅含 ASCII 字母数字与 ._- 的标量可安全原样输出；其余交给 serde_yaml 生成合法标量
    if !s.is_empty()
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        s.to_string()
    } else {
        serde_yaml::to_string(s)
            .map(|v| v.trim_end().to_string()) // serde_yaml 输出带尾部换行，去掉
            .unwrap_or_else(|_| format!("\"{}\"", s.replace('"', "\\\"")))
    }
}
```

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p proxy-core --test formats` 然后 `cargo test -p proxy-core`
Expected: 全部 PASS（原 `clash_yaml_has_proxies_and_groups` 等用例不变——普通名字走安全路径）

- [ ] **Step 5: Commit**

```bash
git add crates/proxy-core/src/formats/clash.rs crates/proxy-core/tests/formats.rs
git commit -m "fix(proxy-core): clash yaml scalar quoting via serde_yaml for hostile inputs"
```

---

### Task 5: Clash YAML 订阅自动识别

**Files:**
- Modify: `crates/proxy-core/src/parser.rs:65-76`（parse_subscription_text）
- Test: `crates/proxy-core/tests/parser_dispatch.rs`

**Interfaces:**
- Consumes: `parse_clash_yaml(text: &str) -> Result<Vec<ProxyNode>, ParseError>`（parser.rs:79，已有）、`parse_lines(text, max_nodes)`（已有）
- Produces: `parse_subscription_text(text: &str, max_nodes: usize) -> (Vec<ProxyNode>, usize)` 行为扩展——检测到行首 `proxies:` 时走 YAML 解析

- [ ] **Step 1: 写失败测试**

在 `crates/proxy-core/tests/parser_dispatch.rs` 追加：

```rust
#[test]
fn subscription_auto_detects_clash_yaml() {
    let yaml = "proxies:\n  - name: \"JP-01\"\n    type: trojan\n    server: 1.2.3.4\n    port: 443\n    password: pass123\n";
    let (nodes, skipped) = parse_subscription_text(yaml, 100);
    assert_eq!(nodes.len(), 1, "clash yaml source must parse");
    assert_eq!(nodes[0].name, "JP-01");
    assert_eq!(nodes[0].kind, Protocol::Trojan);
    assert_eq!(skipped, 0);
}

#[test]
fn subscription_yaml_respects_max_nodes() {
    let mut yaml = String::from("proxies:\n");
    for i in 0..10 {
        yaml.push_str(&format!(
            "  - name: \"N{i}\"\n    type: ss\n    server: 1.2.3.4\n    port: {}\n    cipher: aes-256-gcm\n    password: pw\n",
            8000 + i
        ));
    }
    let (nodes, _) = parse_subscription_text(&yaml, 5);
    assert_eq!(nodes.len(), 5);
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p proxy-core --test parser_dispatch`
Expected: FAIL — 目前 YAML 文本逐行解析得到 0 节点

- [ ] **Step 3: 实现修复**

`parse_subscription_text` 在 base64 检测之前插入 YAML 检测：

```rust
pub fn parse_subscription_text(text: &str, max_nodes: usize) -> (Vec<ProxyNode>, usize) {
    // Clash YAML 订阅：任一行 trim 后以 "proxies:" 开头则按 YAML 解析（在 base64 尝试之前，
    // 避免 YAML 文本被误当 base64；解析失败回退逐行）
    if text.lines().any(|l| l.trim_start().starts_with("proxies:")) {
        return match parse_clash_yaml(text) {
            Ok(mut nodes) => {
                nodes.truncate(max_nodes);
                (nodes, 0)
            }
            Err(_) => parse_lines(text, max_nodes),
        };
    }
    // 若文本看起来是纯 base64（无协议前缀），尝试整体解码
    let trimmed = text.trim();
    let looks_base64 = !trimmed.contains("://") && trimmed.len() > 16;
    if looks_base64
        && let Ok(decoded) = decode_base64_url_string(trimmed)
        && (decoded.contains('\n') || decoded.contains("://"))
    {
        return parse_lines(&decoded, max_nodes);
    }
    parse_lines(text, max_nodes)
}
```

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p proxy-core --test parser_dispatch` 然后 `cargo test -p proxy-core`
Expected: 全部 PASS（原 base64/明文用例不受影响——不含 `proxies:` 行）

- [ ] **Step 5: Commit**

```bash
git add crates/proxy-core/src/parser.rs crates/proxy-core/tests/parser_dispatch.rs
git commit -m "feat(proxy-core): auto-detect clash yaml subscriptions"
```

---

### Task 6: 超大输入截断（parser / uri / fetch_source）

**Files:**
- Modify: `crates/proxy-core/src/parser.rs:49-62`（parse_lines）
- Modify: `crates/proxy-core/src/uri.rs:4-20`（decode_base64_url）
- Modify: `crates/server/src/service.rs:71-84`（fetch_source）
- Test: `crates/proxy-core/src/uri.rs` 内嵌测试、`crates/proxy-core/tests/parser_dispatch.rs`

**Interfaces:**
- Consumes: `parse_lines(text, max_nodes)`、`decode_base64_url(s)`、`fetch_source(client, url, timeout) -> Result<String, String>`（签名均不变）
- Produces: 无新接口；常量 `MAX_LINE_LEN`(parser.rs)、`MAX_BASE64_LEN`(uri.rs)、`MAX_BODY_BYTES`(service.rs) 为模块内私有

- [ ] **Step 1: 写失败测试**

在 `crates/proxy-core/src/uri.rs` 的 `mod tests` 追加：

```rust
    #[test]
    fn base64_oversized_rejected() {
        let big = "A".repeat(4 * 1024 * 1024 + 1);
        assert!(decode_base64_url_string(&big).is_err());
    }
```

在 `crates/proxy-core/tests/parser_dispatch.rs` 追加：

```rust
#[test]
fn subscription_oversized_line_skipped() {
    // 单行超过 1MB：跳过并计数，不解析
    let huge = format!("ss://{}@1.2.3.4:8388#N", "A".repeat(1024 * 1024));
    let (nodes, skipped) = parse_subscription_text(&huge, 100);
    assert_eq!(nodes.len(), 0);
    assert_eq!(skipped, 1);
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p proxy-core uri::tests` 和 `cargo test -p proxy-core --test parser_dispatch`
Expected: 两个新测试 FAIL（无长度限制，当前均可通过解析）

- [ ] **Step 3: 实现修复**

`uri.rs` 的 `decode_base64_url` 开头加：

```rust
    // 解码膨胀防护：超过 4MB 的 base64 输入直接拒绝
    const MAX_BASE64_LEN: usize = 4 * 1024 * 1024;
    if s.len() > MAX_BASE64_LEN {
        return Err(ParseError::InvalidBase64(s.to_string()));
    }
```

`parser.rs` 的 `parse_lines` 循环内、`match parse_line` 之前加：

```rust
        // 恶意超大行截断（>1MB）：跳过并计数
        if line.len() > 1024 * 1024 {
            skipped += 1;
            continue;
        }
```

`service.rs` 的 `fetch_source` 替换 body 读取段：

```rust
    const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("read body failed: {e}"))?;
    if bytes.len() > MAX_BODY_BYTES {
        // 超大 body 截断，防止后续 base64 解码/逐行解析内存膨胀
        return Ok(String::from_utf8_lossy(&bytes[..MAX_BODY_BYTES]).into_owned());
    }
    String::from_utf8(bytes.to_vec()).map_err(|_| "body is not valid utf-8".to_string())
```

（原实现为 `resp.text().await.map_err(...)`。）

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p proxy-core` 和 `cargo test -p server --test api_test`
Expected: 全部 PASS（api_test 中 mock 源 body 远小于限制，不受影响）

- [ ] **Step 5: Commit**

```bash
git add crates/proxy-core/src/parser.rs crates/proxy-core/src/uri.rs crates/server/src/service.rs crates/proxy-core/tests/parser_dispatch.rs
git commit -m "fix: truncate oversized subscription lines, base64 input, and fetch bodies"
```

---

### Task 7: wireguard 单节点失败降级（clash 序列化容错）

**Files:**
- Modify: `crates/proxy-core/src/formats/clash.rs:5-26`（serialize_clash）
- Test: `crates/server/tests/api_test.rs`

**Interfaces:**
- Consumes: `proxy_to_clash(n: &ProxyNode) -> Result<String, SerializeError>`（现有，内部函数）
- Produces: `serialize_clash(nodes: &[ProxyNode]) -> Result<String, SerializeError>` 签名不变；行为——单节点序列化失败跳过，不再整体失败

- [ ] **Step 1: 写失败测试**

在 `crates/server/tests/api_test.rs` 追加：

```rust
#[tokio::test]
async fn subscribe_skips_unserializable_node_instead_of_500() {
    // 源包含一个可解析但无法序列化的 wireguard 节点（缺 privateKey）+ 一个正常 ss 节点
    let mock = MockServer::start().await;
    Mock::given(method("GET")).and(path("/sub"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#OK\n\
             wireguard://cHVibGljS2V5MTIz@1.2.3.4:443?publicKey=cHVibGljS2V5MTIz#WG\n",
        ))
        .mount(&mock)
        .await;

    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-wg-skip", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let pool = test_pool(&tmp).await;
    let (sub, admin) = server::db::ensure_tokens(&pool).await.unwrap();
    let url = format!("{}/sub", mock.uri());
    sqlx::query("INSERT INTO sources (url, name, enabled, created_at) VALUES (?, ?, 1, ?)")
        .bind(&url).bind("mock").bind("now").execute(&pool).await.unwrap();
    let cfg = test_config(&tmp);
    let app = server::routes::build_router(pool, cfg, admin).await;

    let resp = app.clone()
        .oneshot(Request::builder()
            .uri(format!("/api/subscribe?token={}&format=clash", sub))
            .body(Body::empty()).unwrap())
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "bad node must not 500 the subscription");
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(body.contains("name: OK"), "good node must survive");
    assert!(!body.contains("WG"), "unserializable node must be skipped");
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p server --test api_test`
Expected: FAIL — 当前 `proxy_to_clash(n)?` 传播错误，响应 500

- [ ] **Step 3: 实现修复**

`serialize_clash` 改为逐节点容错：

```rust
pub fn serialize_clash(nodes: &[ProxyNode]) -> Result<String, SerializeError> {
    let mut out = String::from("mixed-port: 7890\nallow-lan: false\nmode: rule\nlog-level: info\n\n");
    out.push_str("proxies:\n");
    // 逐节点容错：单个节点序列化失败跳过（与 singbox 的 filter_map 行为一致），
    // 防止一个坏节点（如 wireguard 缺 privateKey）拖垮整个订阅
    let mut ok: Vec<(&ProxyNode, String)> = Vec::new();
    for n in nodes {
        if let Ok(line) = proxy_to_clash(n) {
            out.push_str(&line);
            ok.push((n, line));
        }
    }
    if !ok.is_empty() {
        out.push('\n');
        out.push_str("proxy-groups:\n");
        out.push_str("  - name: \"🚀 节点选择\"\n    type: select\n    proxies:\n");
        for (n, _) in &ok {
            out.push_str(&format!("      - {}\n", clash_yaml_str(&n.name)));
        }
        out.push_str("      - DIRECT\n");
        out.push_str("  - name: \"♻️ 自动选择\"\n    type: url-test\n    url: http://www.gstatic.com/generate_204\n    interval: 300\n    proxies:\n");
        for (n, _) in &ok {
            out.push_str(&format!("      - {}\n", clash_yaml_str(&n.name)));
        }
        out.push_str("\nrules:\n  - MATCH,🚀 节点选择\n");
    }
    Ok(out)
}
```

（`ok` 同时复用序列化结果，group 列表只含成功节点。）

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p server --test api_test` 然后 `cargo test --workspace`
Expected: 全部 PASS（`formats.rs` 中 `clash_yaml_has_proxies_and_groups` 等用例不受影响）

- [ ] **Step 5: Commit**

```bash
git add crates/proxy-core/src/formats/clash.rs crates/server/tests/api_test.rs
git commit -m "fix(proxy-core): per-node tolerant clash serialization, skip not 500"
```

---

### Task 8: `/api` 防护补丁（static fallback）

**Files:**
- Modify: `crates/server/src/static.rs:12`（fallback 的 API 判断）
- Test: `crates/server/tests/api_test.rs`

**Interfaces:**
- Consumes: `ApiError::not_found(msg)`（已有）
- Produces: 无新接口；`/api`、`//api/x`、`/api%2F...` 均返回统一 JSON 404

- [ ] **Step 1: 写失败测试**

在 `crates/server/tests/api_test.rs` 追加：

```rust
#[tokio::test]
async fn api_path_variants_return_json_404() {
    let tmp = std::env::temp_dir().join(format!("submerge-test-{}-api-variants", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    // 构造含 index.html 的 dist，确保 SPA 回退存在（若被绕过会返回 HTML 200）
    let dist = tmp.join("web-dist");
    std::fs::create_dir_all(&dist).unwrap();
    std::fs::write(dist.join("index.html"), "<html>sub-merge</html>").unwrap();

    let pool = test_pool(&tmp).await;
    let (_, admin) = server::db::ensure_tokens(&pool).await.unwrap();
    let cfg = AppConfig { web_dist: dist, ..test_config(&tmp) };
    let app = server::routes::build_router(pool, cfg, admin).await;

    for path in ["/api", "//api/admin/sources", "/api%2Fadmin/preview"] {
        let resp = app.clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "path {path} must be JSON 404");
        let ct = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("");
        assert!(ct.contains("application/json"), "path {path} must return JSON, got {ct:?}");
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p server --test api_test api_path_variants`
Expected: FAIL — `/api` 与 `//api/admin/sources` 当前返回 SPA HTML 200

- [ ] **Step 3: 实现修复**

`static.rs` fallback 中的判断：

```rust
    // 未知的 /api 路径（含 /api、//api/x、编码形态）绝不回退到 SPA，返回统一 JSON 404
    let p = uri.path().trim_start_matches('/');
    if p == "api" || p.starts_with("api/") {
        return ApiError::not_found("route not found").into_response();
    }
```

（原代码为 `if uri.path().starts_with("/api/")`，不覆盖 `/api` 与双斜杠形态。）

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p server --test api_test` 然后 `cargo test --workspace`
Expected: 全部 PASS（原 `static_index_served_from_dist` 中 `/../etc/passwd` 与 `/some/spa/route` 用例不受影响——`/some/...` 不以 api 开头）

- [ ] **Step 5: Commit**

```bash
git add crates/server/src/static.rs crates/server/tests/api_test.rs
git commit -m "fix(server): api-404 guard covers /api and //api path variants"
```

---

### Task 9: 移除 shadowtls 死代码 + 更新设计文档

**Files:**
- Modify: `crates/proxy-core/src/model.rs:110-143`（Transport 的 shadow_tls 字段、ShadowTlsConfig 结构体）
- Modify: `crates/proxy-core/src/lib.rs:13`（re-export）
- Modify: `docs/superpowers/specs/2026-08-05-submerge-design.md`（§1 关键决策 #3、§3 模型草图、§9 已确认决策清单）

**Interfaces:**
- Consumes: 无（确认无其他引用——实现后编译验证）
- Produces: `Transport` 减少 `shadow_tls` 字段；`ShadowTlsConfig` 从 crate 公共 API 移除

- [ ] **Step 1: 确认无引用**

Run: `grep -rn "shadow_tls\|ShadowTls" crates/ --include='*.rs'`
Expected: 仅 model.rs（结构体 + 字段 + 注释）与 lib.rs（re-export）；无测试/序列化器引用

- [ ] **Step 2: 写失败测试（编译级）**

无独立测试——本任务的失败信号是编译错误。先做删除，`cargo build` 报错即为"失败"信号。

- [ ] **Step 3: 实现删除**

`model.rs`：
- 删除 `pub shadow_tls: Option<ShadowTlsConfig>, // shadowtls 作为传输扩展` 行（Transport 内）
- 删除整个 `ShadowTlsConfig` 结构体定义（现 137-143 行，含 `// shadowtls 作为传输扩展` 相关注释）
- 删除 `Transport` 上方注释中 "shadowtls 作为传输扩展" 的提法

`lib.rs`：re-export 列表移除 `ShadowTlsConfig`：

```rust
pub use model::{Crypto, GrpcConfig, HttpUpgradeConfig, Protocol, ProxyNode, TlsSettings, Transport, WebsocketConfig};
```

`docs/superpowers/specs/2026-08-05-submerge-design.md`：
- §1 关键决策 #3：改为 "……wireguard（shadowtls 不在第一版范围）"
- §3 中间模型草图中 `Transport` 注释去掉 shadowtls
- §9 已确认决策清单协议范围行：去掉 "shadowtls 传输扩展"

- [ ] **Step 4: 运行确认通过**

Run: `cargo build --workspace && cargo test --workspace`
Expected: 编译通过，全部测试 PASS

- [ ] **Step 5: Commit**

```bash
git add crates/proxy-core/src/model.rs crates/proxy-core/src/lib.rs docs/superpowers/specs/2026-08-05-submerge-design.md
git commit -m "refactor(proxy-core): remove shadowtls dead code, drop from design spec"
```

---

### Task 10: README.md

**Files:**
- Create: `README.md`（仓库根）

**Interfaces:**
- Consumes: 设计文档 §5.1 内容清单
- Produces: `README.md`——项目文档

- [ ] **Step 1: 编写 README**

按以下结构创建 `README.md`（内容参考 `docs/superpowers/specs/2026-08-05-submerge-design.md` 与 `docs/superpowers/specs/2026-08-06-hardening-design.md`）：

```markdown
# sub-merge

订阅链接聚合与转换工具：聚合多个订阅源，实时并发拉取并合并为一个订阅，
统一输出 Clash YAML / V2Ray base64 / Sing-box JSON 三种格式。小圈子自用，token 鉴权。

## 功能
- 聚合多个订阅源（URL），并发拉取（默认并发 8，单源超时 15s），单源失败自动跳过
- 11 种协议解析：ss、ssr、socks5、http、vmess、vless、trojan、hysteria、hysteria2、tuic、wireguard
- 3 种输出格式：Clash / V2Ray / Sing-box
- 输入支持：V2Ray base64 订阅、明文 URI 列表、Clash YAML（proxies 段）
- 管理界面（WASM）：订阅源 CRUD、转换预览、订阅链接复制、token 轮换

## 快速开始
# 依赖：Rust 1.97+、dx (dioxus-cli 0.8.0-alpha.1)
make run          # 构建前端并启动（首次运行自动建库并生成 token）
# 默认监听 :8080；admin token 打印在日志（debug 级别）或查 DB：
sqlite3 submerge.db "SELECT * FROM settings;"

## Docker 部署
docker build -t sub-merge .
docker run -d --name sub-merge -p 8080:8080 -v submerge-data:/app/data sub-merge

## 环境变量
| 变量 | 默认值 | 说明 |
|------|--------|------|
| PORT | 8080 | 监听端口 |
| DATABASE_PATH | ./submerge.db | SQLite 文件路径 |
| CONCURRENCY | 8 | 并发拉取上限 |
| TIMEOUT_SECS | 15 | 单源超时 |
| MAX_NODES | 2000 | 节点总数上限 |
| WEB_DIST | ./web/dist | 前端静态资源目录 |

## API
### 订阅接口
GET /api/subscribe?token=<订阅token>&format=clash|v2ray|singbox

### 管理接口（Authorization: Bearer <管理token>）
| 方法 | 路径 | 说明 |
|------|------|------|
| GET/POST | /api/admin/sources | 列表 / 添加订阅源 |
| PUT/DELETE | /api/admin/sources/{id} | 更新（url/name/enabled）/ 删除 |
| POST | /api/admin/sources/{id}/refresh | 手动刷新单源 |
| GET | /api/admin/preview | 转换结果预览 |
| GET/PUT | /api/admin/config | 获取配置 / 轮换 token |

错误统一返回 `{"error":{"code":"...","message":"..."}}`。

### 注意：V2Ray 格式的节点覆盖
`format=v2ray` 输出仅包含 ss/ssr/vmess/vless/trojan/tuic 节点；
socks5、http、hysteria、hysteria2、wireguard 节点在此格式被跳过（请使用 clash 或 singbox 格式）。

## 开发
- `make build-web`：构建前端 WASM（dx build --web）
- `make build-server`：构建后端（release）
- `make smoke`：端到端冒烟测试
- `cargo test --workspace`：全部单元/集成测试

## 架构
axum 服务（Rust） + proxy-core 协议库（纯逻辑、无 IO） + Dioxus WASM 管理界面 + SQLite 存储。
```

- [ ] **Step 2: 核对内容**

对照设计文档 §5.1 清单逐项确认（架构、快速开始、Docker、环境变量表、API、token 获取、v2ray 覆盖说明、测试说明）。

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: project README"
```

---

### Task 11: CI 流水线

**Files:**
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: 现有 Makefile 目标（build-web/build-server）
- Produces: GitHub Actions workflow（fmt + clippy + test + web 构建 + docker 构建）

- [ ] **Step 1: 编写 workflow**

创建 `.github/workflows/ci.yml`：

```yaml
name: CI

on:
  push:
    branches: [master, main]
  pull_request:

jobs:
  check:
    name: fmt + clippy + test
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          toolchain: 1.97
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - name: fmt
        run: cargo fmt --all -- --check
      - name: clippy
        run: cargo clippy --workspace --all-targets -- -D warnings
      - name: test
        run: cargo test --workspace

  web:
    name: web (dx build)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          toolchain: 1.97
          targets: wasm32-unknown-unknown
      - uses: Swatinem/rust-cache@v2
      - name: install dx
        run: cargo install dioxus-cli --version 0.8.0-alpha.1
      - name: build web
        working-directory: crates/server/web
        run: |
          dx build --web
          test -d target/dx/submerge-web/debug/web/public || exit 1

  docker:
    name: docker build
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: build image
        run: docker build -t sub-merge .
```

- [ ] **Step 2: 本地验证 workflow 中的命令**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 通过（若 clippy 有 warnings，先修复再提交本任务；修复属于本任务范围）

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: github actions workflow (fmt, clippy, test, web, docker)"
```

---

### Task 12: 全量验证与收尾

**Files:**
- 无新文件

**Interfaces:**
- Consumes: 全部任务的产出

- [ ] **Step 1: 全量测试**

Run: `cargo test --workspace`
Expected: 全部 PASS（约 95+ 个测试）

- [ ] **Step 2: 冒烟测试**

Run: `make smoke`
Expected: 9 步全部通过（依赖 dx 已安装；若未安装 dx，跳过本步并在 README 注明）

- [ ] **Step 3: 对照设计文档逐项核对**

对照 `docs/superpowers/specs/2026-08-06-hardening-design.md` 的 §3/§4/§5 清单逐项确认已实现：
- [ ] 3.1 hy2 `?` 修复
- [ ] 3.2 trojan `?` 修复
- [ ] 3.3 trojan TLS 默认
- [ ] 3.4 clash 标量转义
- [ ] 4.1 Clash YAML 自动识别
- [ ] 4.2 超大输入截断（1MB 行 / 4MB base64 / 16MB body）
- [ ] 4.3 wireguard 单节点降级
- [ ] 4.4 `/api` 防护补丁
- [ ] 5.1 README
- [ ] 5.2 CI
- [ ] 5.3 测试增强（各 Task 内已含）

- [ ] **Step 4: 提交收尾**

若 Step 1-3 有任何修复，提交；否则无需提交：
```bash
git add -A
git commit -m "chore: final verification" || true
```
