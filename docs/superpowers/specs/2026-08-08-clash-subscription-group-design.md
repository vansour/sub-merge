# Clash 订阅组模式 + 默认配置管理设计

日期：2026-08-08
状态：已批准（待实施）

## 背景与目标

现状 `/subscribe/{name}?format=clash` 把成员源解析成节点再序列化为 Clash YAML（节点经 `ProxyNode` 模型转换会丢失 reality flow/sid/pbk 等原始参数，且输出固定不可配）。用户要求：

1. **clash 输出替换为订阅组模式（默认行为）**：不再输出解析节点，改为 `proxy-providers` 引用 sub-merge 自己的组合订阅聚合链接，Clash 客户端直接拉取——原始参数完整保留，节点更新由 Clash 按 interval 自动拉取
2. **前端新增菜单管理 clash 默认配置**：dns、分流 rules 及头部字段等以 YAML 文本编辑，系统自动追加 providers/groups 段

## 输出形态（`GET /subscribe/{name}?format=clash`）

```yaml
mixed-port: 7890
allow-lan: false
mode: rule
log-level: info
# ...用户模板自定义段（dns/rules 等）...

proxy-providers:
  {组合名}:
    type: http
    url: "http://{请求Host}/subscribe/{name}?format=v2ray"
    interval: 3600
    path: ./providers/{name}.yaml
    health-check:
      enable: true
      url: http://www.gstatic.com/generate_204
      interval: 300

proxy-groups:
  - name: "🚀 节点选择"
    type: select
    use:
      - {组合名}
    proxies:
      - DIRECT
  - name: "♻️ 自动选择"
    type: url-test
    url: http://www.gstatic.com/generate_204
    interval: 300
    use:
      - {组合名}
```

（模板缺省 = 上述头部四行；rules 属用户模板范围，默认模板含 `rules:\n  - MATCH,🚀 节点选择` 供参考）

**关键语义：**
- provider url 指向 sub-merge 自己的组合订阅链接（`format=v2ray` 聚合订阅）——成员源（remote + single）由 sub-merge 解析聚合，Clash 经 provider 直接拉取；静态节点天然包含
- **节点→clash 转换不再执行**（mihomo 的 http provider 支持 v2ray base64 订阅输入）
- provider key 用组合名（`[A-Za-z0-9-_]` 已限定）
- 更新链路：源变化 → Clash 按 interval 重新拉聚合订阅 → health-check 剔除不可用节点

## 后端实现

### 模板存储与 API

- `settings` 表新增 key `clash_template`（YAML 文本）；读取缺省返回默认模板（不初始化写入）
- db.rs 重新加回 `get_setting`/`set_setting`（settings 表仍在）
- 新路由 `routes/clash_config.rs`（Bearer 鉴权）：
  - `GET /admin/clash-config` → `{"template": "..."}`
  - `PUT /admin/clash-config` → body `{"template": "..."}`；YAML 必须可解析（serde_yaml_ng），否则 400；保存成功返回 `{"template": "..."}`

### 输出链路（subscribe.rs clash 分支）

```
读模板（缺省默认）→ proxy-core serialize_clash_subscription(template, provider_key, provider_url)
  = serde_yaml_ng 解析模板为 Value
  → 插入 proxy-providers / proxy-groups 两段（系统自动追加，覆盖模板同键）
  → 序列化输出
```

- proxy-core `formats/clash.rs` 新增：`serialize_clash_subscription(template: &str, provider_key: &str, provider_url: &str) -> Result<String, SerializeError>`
- 系统段 = proxy-providers + proxy-groups；头部/dns/rules 全部来自模板
- provider url：`{scheme}://{请求Host}/subscribe/{name}?format=v2ray`（scheme 由 `X-Forwarded-Proto` 或默认 http；Host 缺失 → 400）
- v2ray/singbox 分支照旧（解析输出，不受影响）

### 边界

| 场景 | 处理 |
|------|------|
| 模板 YAML 非法（PUT） | 400 |
| 模板含 providers/groups 键 | 被系统段覆盖（解析合并，输出合法） |
| 空成员组合 | 照常输出（provider 指向空聚合订阅） |
| Host 缺失 | 400 |
| v2ray/singbox | 照旧解析输出 |

## 前端

**导航**：新增一级叶子「Clash 配置」（与「配置」并列）。叶子索引：`0=本地 1=远程 2=组合 3=Clash 配置 4=配置`；默认 tab=0 不变；MainShell required_units 同步。

**页面** `components/clash_config.rs`：
- DataStore 新增 `clash_config` 单元（第 5 个单元，`UnitKey::ClashConfig`）
- YAML 文本域（textarea，mono 字体）+「保存」按钮（PUT 400 显示错误）+ 说明文案（「头部/dns/rules 在此编辑；proxy-providers 与 proxy-groups 由系统自动追加」）
- 保存成功 → toast + refresh 单元回写

**组合订阅页**复制链接按钮不变（format=clash 默认即订阅组模式）。

## 测试

- proxy-core：`serialize_clash_subscription` 单测——默认模板合并后含 providers/groups、url 正确、模板自定义段（dns/rules）保留、模板含 providers 键被覆盖、模板非法返回 Err
- server 集成（api_test.rs）：
  - clash 输出含 `proxy-providers:` + provider url 拼请求 Host + `use:` 引用
  - `GET/PUT /admin/clash-config` 鉴权、PUT 合法保存回读一致、PUT 非法 YAML 400
  - v2ray/singbox 既有测试保持（不受影响）
  - Host 缺失 → 400
- ui-check.py：新增 `clash_config` 场景（导航 → 编辑模板 → 保存 → 断言）；既有场景核对（tab 索引变化用文本匹配点击，不受影响；nav_preload 慢路径「配置」断言仍有效）
- 浏览器人工核对：订阅组输出在 mihomo/Clash 客户端实际加载验证

## 文档

- README：API 表加 clash-config 两行；clash 格式说明更新（订阅组模式）
- CLAUDE.md：架构节提 clash_config 单元

## 不做的事（YAGNI）

- provider 的 url 指向源订阅直连（已确认指向 sub-merge 聚合链接）
- 结构化表单编辑模板（YAML 文本已够）
- 模板版本历史/回滚
- 代理组自定义（groups 由系统生成，固定两组的形态不变）
