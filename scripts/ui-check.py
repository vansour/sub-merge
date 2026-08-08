#!/usr/bin/env python3
# 前端 UI 行为检查(CDP 驱动 headless chrome)。
# 前置:1) server 运行在 :18080(无需预设 token;脚本会自动完成首次创建管理员与登录)
#       2) chrome-headless-shell --headless --no-sandbox --remote-debugging-port=9222 \
#          --remote-allow-origins=* about:blank
# 用法:python3 scripts/ui-check.py <scenario> [url]
import json, sys, time, urllib.request, urllib.parse
import websocket

URL = sys.argv[2] if len(sys.argv) > 2 else "http://127.0.0.1:18080"
CDP = "http://127.0.0.1:9222"

# 测试用管理员账号:首次运行经 /admin/setup 创建,后续场景经 /admin/login 拿会话。
# 会话 token 存全局 SESSION_TOKEN,供 API 直调函数使用。
ADMIN_USER = "ui"
ADMIN_PASS = "ui-pass-12345"
SESSION_TOKEN = None

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
        # 超时/断连返回 None(假值):wait_until/assert_true 会自然 FAIL,不得返回真值。
        return None

def api_login(username, password):
    """urllib 直调 /admin/login,返回会话 token(失败抛异常)。"""
    req = urllib.request.Request(URL + "/admin/login", method="POST",
                                 data=json.dumps({"username": username, "password": password}).encode(),
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=5) as r:
        return json.loads(r.read())["token"]

def ensure_session():
    """API 层确保管理员存在并拿到会话 token(setup-status → 必要时 setup → login)。

    只设置全局 SESSION_TOKEN,不触碰页面;seed/cleanup 等 API 直调函数在开头调用。
    已有会话时直接复用(单场景进程内会话跨 API 调用有效,改密场景收尾会刷新)。"""
    global SESSION_TOKEN
    if SESSION_TOKEN:
        return
    with urllib.request.urlopen(URL + "/admin/setup-status", timeout=5) as r:
        needs_setup = json.loads(r.read())["needs_setup"]
    if needs_setup:
        req = urllib.request.Request(URL + "/admin/setup", method="POST",
                                     data=json.dumps({"username": ADMIN_USER, "password": ADMIN_PASS,
                                                      "password_confirm": ADMIN_PASS}).encode(),
                                     headers={"Content-Type": "application/json"})
        urllib.request.urlopen(req, timeout=5)
    SESSION_TOKEN = api_login(ADMIN_USER, ADMIN_PASS)

def login(ws):
    """页面登录:API 层拿真实会话 token → 写入 localStorage(submerge_admin_session) → 刷新。

    先固定桌面视口(CDP 仿真,登录刷新前生效):既有场景的导航断言(叶子常驻/spinner/
    分组折叠)均以桌面展开态为前提——移动端(<900px)分组默认折叠,叶子不在 DOM
    (响应式设计使然,非缺陷)。responsive 场景在 login 后自行切换各断点,不受影响。"""
    cmd(ws, "Emulation.setDeviceMetricsOverride", {"width": 1280, "height": 800, "deviceScaleFactor": 1, "mobile": False})
    ensure_session()
    cmd(ws, "Page.enable"); cmd(ws, "Runtime.enable")
    time.sleep(2)
    for _ in range(20):
        if ev(ws, "document.readyState") == "complete":
            break
        time.sleep(0.5)
    ev(ws, "localStorage.setItem('submerge_admin_session','%s')" % SESSION_TOKEN)
    cmd(ws, "Page.reload")
    time.sleep(2.5)

def nav_el(ws, label):
    return ev(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('%s'))!==undefined" % label)

def nav_loading(ws, label):
    # 注意:querySelector 未命中返回 null,null !== undefined 恒真,必须 !! 包裹。
    return ev(ws, "!!(Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('%s'))?.querySelector('.spinner'))" % label)

def nav_active(ws, label):
    return ev(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('%s')).classList.contains('active')" % label)

def click_nav(ws, label):
    ev(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('%s')).click()" % label)
    time.sleep(0.3)

# 精确匹配版本：导航含「Clash 配置」与「配置」两个按钮后，includes('配置') 会先命中
# 「Clash 配置」（DOM 顺序 tab=3 在前）——叶子断言/点击一律用 trim 后全等匹配。
def nav_button(ws, label):
    return ev(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.trim()==='%s')!==undefined" % label)

def nav_loading_exact(ws, label):
    return ev(ws, "!!(Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.trim()==='%s')?.querySelector('.spinner'))" % label)

def nav_active_exact(ws, label):
    return ev(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.trim()==='%s').classList.contains('active')" % label)

def click_nav_exact(ws, label):
    ev(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.trim()==='%s').click()" % label)
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
    """经 API 种 n 个 single 源(需 server 已运行;会话经 ensure_session 自动获取)。"""
    import urllib.request as u
    ensure_session()
    for i in range(n):
        req = u.Request(URL + "/admin/sources", method="POST",
                        data=json.dumps({"name": "s%d" % i,
                                         "url": "vless://e99a8e5a-6b2b-4a1d-9c5f-1a2b3c4d5e6f@1.2.3.%d:443#n%d" % (i + 1, i),
                                         "kind": "single"}).encode(),
                        headers={"Authorization": "Bearer " + SESSION_TOKEN, "Content-Type": "application/json"})
        u.urlopen(req, timeout=5)

def cleanup_source(name):
    """经 API 删除同名源(幂等)。sources_crud 每次运行新增 crud-test,
    尾部自清理,避免跨次运行累积(否则 DB 里同名源越来越多)。"""
    import urllib.request as u
    ensure_session()
    req = u.Request(URL + "/admin/sources", headers={"Authorization": "Bearer " + SESSION_TOKEN})
    with u.urlopen(req, timeout=5) as r:
        items = json.loads(r.read())
    for it in items:
        if it.get("name") == name:
            u.urlopen(u.Request(URL + "/admin/sources/%d" % it["id"], method="DELETE",
                                headers={"Authorization": "Bearer " + SESSION_TOKEN}), timeout=5)

def cleanup_combined(name):
    """经 API 删除同名组合(幂等)。combined_subs.name 有 UNIQUE 约束,
    场景重跑前须先清掉同名旧数据,否则保存会 400 失败。"""
    import urllib.request as u
    ensure_session()
    req = u.Request(URL + "/admin/combineds", headers={"Authorization": "Bearer " + SESSION_TOKEN})
    with u.urlopen(req, timeout=5) as r:
        items = json.loads(r.read())
    for it in items:
        if it.get("name") == name:
            u.urlopen(u.Request(URL + "/admin/combineds/%d" % it["id"], method="DELETE",
                                headers={"Authorization": "Bearer " + SESSION_TOKEN}), timeout=5)

# 在新文档创建时注入(早于 app 脚本):MutationObserver 记录加载瞬态是否出现过。
# 本机 /admin/preview 对 single 源即时返回(不实际拉取),loading 窗口仅 ~100ms,
# 固定 sleep 后查询会错过,故用观察者标记替代瞬时查询,断言语义不变。
OBSERVER_JS = """
window.__ui = { saw_spinner: false, saw_loading: false };
try {
  // 注入脚本在文档创建早期运行,documentElement 尚不存在,observe(document) 通用。
  new MutationObserver(function () {
    if (!window.__ui.saw_spinner && document.querySelector('nav button .spinner')) window.__ui.saw_spinner = true;
    if (!window.__ui.saw_loading && document.querySelector('.page-loading')) window.__ui.saw_loading = true;
  }).observe(document, { childList: true, subtree: true });
} catch (e) {}
"""

def scenario_nav_preload(ws):
    """首次切换:旧页保持 + 菜单项转圈 → 就绪后切换;已加载页回访秒开;分组折叠/展开。

    桌面视口由 login() 统一固定(叶子断言依赖桌面展开态,见 login 注释)。"""
    seed_sources(ws, 1)
    cmd(ws, "Page.addScriptToEvaluateOnNewDocument", {"source": OBSERVER_JS})
    login(ws)
    # 初始页=本地订阅(第一个叶子),预载 sources 单元
    assert_true(wait_until(ws, "window.__ui.saw_spinner===true", timeout=15), "初始加载中菜单项转圈")
    assert_true(wait_until(ws, "window.__ui.saw_loading===true", timeout=3), "初始加载内容区显示全页 loading")
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.trim()==='本地订阅').classList.contains('active')"), "本地订阅就绪后激活")
    # 分组折叠:点「单条订阅」收起 → 本地订阅不可见 → 再展开
    ev(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('单条订阅')).click()")
    time.sleep(0.3)
    assert_true(not nav_button(ws, "本地订阅"), "折叠后三级菜单隐藏")
    ev(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('单条订阅')).click()")
    time.sleep(0.3)
    assert_true(nav_button(ws, "本地订阅"), "展开后三级菜单可见")
    # 回访秒开:切远程订阅(同 sources 单元缓存) → 立即切换
    click_nav_exact(ws, "远程订阅")
    assert_true(nav_active_exact(ws, "远程订阅"), "同单元切换秒开")
    assert_true(not nav_loading_exact(ws, "远程订阅"), "秒开路径无转圈")
    # 慢路径(点按切换):注入 4s 网络延迟 → 点「配置」(首个请求,无缓存) →
    # 加载窗口内旧页保持可见 + 菜单项转圈 + 未提前切换 → 就绪后切换完成。
    cmd(ws, "Network.enable")
    cmd(ws, "Network.emulateNetworkConditions",
        {"offline": False, "latency": 4000, "downloadThroughput": -1, "uploadThroughput": -1})
    click_nav_exact(ws, "配置")
    time.sleep(1.0)
    # 旧页保持:nav 文案恒在 body 内(远程订阅菜单项常在),不能只查 body 文本——
    # 断言激活态 + 页面内容(订阅源列表徽章,配置页无此结构)。
    assert_true(ev(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.trim()==='远程订阅').classList.contains('active')"), "慢加载期间旧页(远程订阅)保持激活")
    assert_true(ev(ws, "!!document.querySelector('.card h2 + .badge')"), "慢加载期间旧页内容(订阅源列表徽章)保持可见")
    assert_true(nav_loading_exact(ws, "配置"), "慢加载期间目标菜单项转圈")
    assert_true(not nav_active_exact(ws, "配置"), "慢加载期间未提前切换")
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.trim()==='配置').classList.contains('active')", timeout=15), "就绪后切换完成")
    cmd(ws, "Network.emulateNetworkConditions",
        {"offline": False, "latency": 0, "downloadThroughput": -1, "uploadThroughput": -1})

def scenario_sources_crud(ws):
    """本地订阅页添加源 → 计数徽章 +1(原概览统计断言迁移)。"""
    login(ws)
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.trim()==='本地订阅').classList.contains('active')"), "本地订阅就绪")
    # 记录添加前的源数:DB 会跨场景累积(seed/重复运行),断言用 N+1 而非写死。
    # 徽章是「订阅源列表」卡片 h2.card-title 的相邻兄弟(.card h2 + .badge)。
    n0 = ev(ws, "parseInt(Array.from(document.querySelectorAll('.card h2 + .badge, .card-title + .badge'))[0]?.textContent ?? '0', 10)")
    assert_true(isinstance(n0, int), "读取到添加前源数徽章")
    # 添加表单:URL + 名称 两个 input + 添加按钮(表单类型固定为页面 kind,无类型下拉)
    ev(ws, "(()=>{const ins=document.querySelectorAll('.form-row input');ins[0].value='vless://e99a8e5a-6b2b-4a1d-9c5f-1a2b3c4d5e6f@9.9.9.9:443#crud-test';ins[0].dispatchEvent(new Event('input',{bubbles:true}));ins[1].value='crud-test';ins[1].dispatchEvent(new Event('input',{bubbles:true}));})()")
    ev(ws, "Array.from(document.querySelectorAll('.form-row button')).find(b=>b.textContent.includes('添加')).click()")
    time.sleep(0.8)
    assert_true(wait_until(ws, "document.body.innerText.includes('crud-test')"), "添加后列表出现新源")
    assert_true(wait_until(ws, "parseInt(Array.from(document.querySelectorAll('.card h2 + .badge, .card-title + .badge'))[0]?.textContent ?? '0', 10) === %d" % (n0 + 1)), "计数徽章 +1(缓存回写)")
    cleanup_source("crud-test")

def scenario_config_password(ws):
    """配置页:账号卡片渲染用户名(缓存读取);「订阅输出」开关切换保存与刷新回读;
    改密后全部会话失效被踢回登录页;新密码重新登录。

    收尾(finally)用 API 把密码改回 ui-pass-12345 并刷新 SESSION_TOKEN,
    使后续场景不受影响(改密后任一步断言失败也执行恢复)。"""
    global SESSION_TOKEN
    NEW_PASS = "ui-pass-67890"
    login(ws)
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.trim()==='本地订阅').classList.contains('active')"), "本地订阅就绪")
    # 前置自愈:把 v2ray_base64 恢复为默认 true(上次运行中途失败残留也能自愈)。
    # 配置单元此时未加载(预载仅 tab=0 的 sources),恢复后再进配置页读到新值。
    ensure_session()
    import urllib.request as u
    u.urlopen(u.Request(URL + "/admin/config", method="PUT",
                        data=json.dumps({"v2ray_base64": True}).encode(),
                        headers={"Authorization": "Bearer " + SESSION_TOKEN,
                                 "Content-Type": "application/json"}), timeout=5)
    click_nav_exact(ws, "配置")
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.trim()==='配置').classList.contains('active')"), "配置就绪")
    # 账号卡片:用户名来自 DataStore 缓存(GET /admin/config),渲染为 .token-row .token-value
    assert_true(wait_until(ws, "document.querySelector('.card .token-value')?.textContent === '%s'" % ADMIN_USER), "账号卡片渲染用户名 ui")
    # —— 订阅输出卡片:开关默认勾选 → 切换关 → 保存 → toast ——
    # (改密会登出,开关断言全部放在改密之前)
    assert_true(wait_until(ws, "!!document.querySelector('.switch-row')"), "订阅输出卡片出现")
    assert_true(wait_until(ws, "document.querySelector('.switch-row input').checked === true"), "开关默认勾选(base64 开)")
    ev(ws, "document.querySelector('.switch-row input').click()")
    assert_true(wait_until(ws, "document.querySelector('.switch-row input').checked === false"), "点击后开关取消勾选")
    ev(ws, "Array.from(document.querySelectorAll('.card button')).find(b=>b.textContent.includes('保存设置')).click()")
    assert_true(wait_until(ws, "document.body.innerText.includes('订阅输出设置已保存')", timeout=10), "保存成功 toast")
    # 恢复默认:切回勾选并保存,不污染共享 DB(默认行为 = base64 开)
    ev(ws, "document.querySelector('.switch-row input').click()")
    assert_true(wait_until(ws, "document.querySelector('.switch-row input').checked === true"), "恢复勾选")
    ev(ws, "Array.from(document.querySelectorAll('.card button')).find(b=>b.textContent.includes('保存设置')).click()")
    assert_true(wait_until(ws, "document.body.innerText.includes('订阅输出设置已保存')", timeout=10), "恢复保存 toast")
    # 刷新回读:开关状态从服务端保持(默认开)
    cmd(ws, "Page.reload")
    time.sleep(2.5)
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.trim()==='配置')!==undefined", timeout=15), "刷新后导航就绪")
    click_nav_exact(ws, "配置")
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.trim()==='配置').classList.contains('active')"), "刷新后配置就绪")
    assert_true(wait_until(ws, "!!document.querySelector('.switch-row input') && document.querySelector('.switch-row input').checked === true", timeout=10), "刷新后开关状态保持(base64 开)")
    # 三个密码输入按渲染顺序(账号卡片在前):当前密码/新密码/确认新密码
    ev(ws, "(()=>{const ins=document.querySelectorAll('.card input');ins[0].value='%s';ins[0].dispatchEvent(new Event('input',{bubbles:true}));ins[1].value='%s';ins[1].dispatchEvent(new Event('input',{bubbles:true}));ins[2].value='%s';ins[2].dispatchEvent(new Event('input',{bubbles:true}));})()" % (ADMIN_PASS, NEW_PASS, NEW_PASS))
    ev(ws, "Array.from(document.querySelectorAll('button')).find(b=>b.textContent.includes('修改密码')).click()")
    try:
        # 改密成功后服务端使全部会话失效,本地清除会话 → 回登录页
        assert_true(wait_until(ws, "!!document.querySelector('.login-card')", timeout=10), "改密后被踢回登录页")
        # 登录页探测 setup 状态后出现登录表单:用新密码走页面表单重新登录(全链路)
        assert_true(wait_until(ws, "!!Array.from(document.querySelectorAll('.login-card button')).find(b=>b.textContent.includes('登录'))", timeout=10), "登录页就绪(显示登录表单)")
        ev(ws, "(()=>{const ins=document.querySelectorAll('.login-card input');ins[0].value='%s';ins[0].dispatchEvent(new Event('input',{bubbles:true}));ins[1].value='%s';ins[1].dispatchEvent(new Event('input',{bubbles:true}));})()" % (ADMIN_USER, NEW_PASS))
        ev(ws, "Array.from(document.querySelectorAll('.login-card button')).find(b=>b.textContent.includes('登录')).click()")
        assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.trim()==='本地订阅').classList.contains('active')", timeout=15), "新密码登录成功(进入本地订阅)")
        # 页面登录已写入新会话 → 同步到全局 SESSION_TOKEN,供 API 恢复用
        tok = ev(ws, "localStorage.getItem('submerge_admin_session')")
        assert_true(bool(tok), "新会话 token 已写入 localStorage")
        SESSION_TOKEN = tok
    finally:
        # 恢复:密码可能已改为 NEW_PASS(改密成功)也可能未改(断言中途失败),两种情况都覆盖;
        # 改回 ADMIN_PASS 后重新登录刷新 SESSION_TOKEN,不留毒害后续场景。
        import urllib.request as u
        restored = False
        for cand, is_new in ((ADMIN_PASS, False), (NEW_PASS, True)):
            try:
                tok = api_login(ADMIN_USER, cand)
            except Exception:
                continue
            if is_new:
                req = u.Request(URL + "/admin/config", method="PUT",
                                data=json.dumps({"change_password": {"old": cand, "new": ADMIN_PASS}}).encode(),
                                headers={"Authorization": "Bearer " + tok, "Content-Type": "application/json"})
                u.urlopen(req, timeout=5)
            SESSION_TOKEN = api_login(ADMIN_USER, ADMIN_PASS)
            restored = True
            break
        if not restored:
            print("WARN: 密码恢复失败(新旧密码均无法登录),后续场景可能受影响")

def scenario_combineds(ws):
    """组合订阅:新建 → 列表出现;保存后缓存 refresh 回写。"""
    cleanup_combined("c-test")
    seed_sources(ws, 1)
    login(ws)
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.trim()==='本地订阅').classList.contains('active')"), "本地订阅就绪")
    click_nav_exact(ws, "组合订阅")
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.trim()==='组合订阅').classList.contains('active')"), "组合订阅就绪")
    ev(ws, "Array.from(document.querySelectorAll('button')).find(b=>b.textContent.includes('新建组合')).click()")
    time.sleep(0.5)
    ev(ws, "(()=>{const el=document.querySelector('.modal input');const s=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set;s.call(el,'c-test');el.dispatchEvent(new Event('input',{bubbles:true}));})()")
    ev(ws, "document.querySelector('.member-row input').click()")
    time.sleep(0.3)
    ev(ws, "Array.from(document.querySelectorAll('.modal-actions button')).find(b=>b.textContent.includes('保存')).click()")
    assert_true(wait_until(ws, "document.body.innerText.includes('c-test')"), "保存后列表出现 c-test")

def scenario_preview(ws):
    """预览弹窗：订阅源行内预览 → 全屏弹窗节点渲染 → 关闭；组合页行内预览同样。

    组合部分：c-test 已在列表则直接点行内预览，缺失则复用 scenario_combineds 的
    创建流程（新建组合 → 名称 → 勾选成员 → 保存）再预览。c-test 恒 1 成员 1 节点
    （所有 seed 源均为单节点 vless），弹窗行数确定性 === 1。"""
    seed_sources(ws, 1)
    login(ws)
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.trim()==='本地订阅').classList.contains('active')"), "本地订阅就绪")
    # 行内三按钮存在（预览/编辑/删除）
    assert_true(ev(ws, "['预览','编辑','删除'].every(t=>Array.from(document.querySelectorAll('.table-wrap-sources .cell-actions .btn')).some(b=>b.textContent.trim()===t))") is True, "行内三按钮（预览/编辑/删除）存在")
    # 行内预览 → 全屏弹窗 → 节点渲染 → 关闭
    ev(ws, "Array.from(document.querySelectorAll('.table-wrap-sources .cell-actions .btn')).find(b=>b.textContent.includes('预览')).click()")
    assert_true(wait_until(ws, "!!document.querySelector('.fullscreen-modal')"), "全屏预览弹窗出现")
    assert_true(wait_until(ws, "!!document.querySelector('.fullscreen-modal .table-wrap tbody tr')", timeout=10), "预览节点渲染")
    ev(ws, "Array.from(document.querySelectorAll('.fullscreen-modal .btn')).find(b=>b.textContent.includes('关闭')).click()")
    assert_true(wait_until(ws, "!document.querySelector('.fullscreen-modal')"), "关闭后弹窗消失")
    # 组合页：行内预览同样
    click_nav_exact(ws, "组合订阅")
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.trim()==='组合订阅').classList.contains('active')"), "组合订阅就绪")
    if not ev(ws, "!!Array.from(document.querySelectorAll('.combined-row')).find(r=>r.querySelector('.combined-name')?.textContent==='c-test')"):
        # c-test 缺失：复用 scenario_combineds 创建流程
        ev(ws, "Array.from(document.querySelectorAll('button')).find(b=>b.textContent.includes('新建组合')).click()")
        time.sleep(0.5)
        ev(ws, "(()=>{const el=document.querySelector('.modal input');const s=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set;s.call(el,'c-test');el.dispatchEvent(new Event('input',{bubbles:true}));})()")
        ev(ws, "document.querySelector('.member-row input').click()")
        time.sleep(0.3)
        ev(ws, "Array.from(document.querySelectorAll('.modal-actions button')).find(b=>b.textContent.includes('保存')).click()")
        assert_true(wait_until(ws, "document.body.innerText.includes('c-test')"), "保存后组合列表出现 c-test")
    # c-test 行内预览 → 弹窗节点（1 成员 1 节点）→ 关闭
    ev(ws, "(()=>{const r=Array.from(document.querySelectorAll('.combined-row')).find(r=>r.querySelector('.combined-name')?.textContent==='c-test');Array.from(r.querySelectorAll('.actions .btn')).find(b=>b.textContent.includes('预览')).click();})()")
    assert_true(wait_until(ws, "!!document.querySelector('.fullscreen-modal')"), "组合行内预览弹窗出现")
    assert_true(wait_until(ws, "document.querySelectorAll('.fullscreen-modal .table-wrap tbody tr').length === 1", timeout=10), "组合预览节点渲染（1 成员 1 节点）")
    ev(ws, "Array.from(document.querySelectorAll('.fullscreen-modal .btn')).find(b=>b.textContent.includes('关闭')).click()")
    assert_true(wait_until(ws, "!document.querySelector('.fullscreen-modal')"), "组合预览关闭后弹窗消失")

def scenario_preview_failure(ws):
    """预览弹窗失败路径：preview 请求被拦截 → 弹窗显示加载失败 + 重试 → 解除拦截点重试恢复。

    用 CDP 请求拦截（Network.setBlockedURLs）让 /admin/preview 请求失败
    （同已删除 first_load_failure 的手法，无需杀 server）；拦截须在 login()
    触发导航前生效（Network.enable 幂等，login() 内部再启 Page/Runtime 无冲突）。"""
    cmd(ws, "Network.enable")
    cmd(ws, "Network.setBlockedURLs", {"urls": ["*admin/preview*"]})
    seed_sources(ws, 1)
    login(ws)
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.trim()==='本地订阅').classList.contains('active')"), "本地订阅就绪")
    # 行内预览 → 全屏弹窗失败态（empty-title「加载失败」+ 重试按钮，见 preview_modal.rs 失败分支）
    ev(ws, "Array.from(document.querySelectorAll('.table-wrap-sources .cell-actions .btn')).find(b=>b.textContent.includes('预览')).click()")
    assert_true(wait_until(ws, "!!document.querySelector('.fullscreen-modal')"), "全屏预览弹窗出现")
    assert_true(wait_until(ws, "!!document.querySelector('.fullscreen-modal') && document.querySelector('.fullscreen-modal').innerText.includes('加载失败')", timeout=10), "弹窗显示加载失败态")
    # 解除拦截 → 点重试 → 节点渲染
    cmd(ws, "Network.setBlockedURLs", {"urls": []})
    ev(ws, "Array.from(document.querySelectorAll('.fullscreen-modal .btn')).find(b=>b.textContent.includes('重试')).click()")
    assert_true(wait_until(ws, "document.querySelectorAll('.fullscreen-modal .table-wrap tbody tr').length > 0", timeout=10), "重试后节点渲染")
    ev(ws, "Array.from(document.querySelectorAll('.fullscreen-modal .btn')).find(b=>b.textContent.includes('关闭')).click()")
    assert_true(wait_until(ws, "!document.querySelector('.fullscreen-modal')"), "关闭后弹窗消失")

def scenario_responsive(ws):
    """响应式适配：视口矩阵无水平溢出 + 移动端抽屉可达性（导航全可达）+ 跨断点 resize 跟随。

    前置:server 端口可经命令行 URL 参数覆盖(默认 18080);CDP 视口仿真会触发页面
    matchMedia change + resize 事件,App 的 is_mobile 信号随之跟随(无需刷新页面)。"""
    seed_sources(ws, 1)
    login(ws)
    # 1) 视口矩阵:无文档级水平溢出(320 宽度下订阅源表走卡片化布局,亦在覆盖范围)
    for name, w, h in [("desktop", 1280, 800), ("tablet", 768, 1024), ("phone", 390, 844), ("small", 320, 568)]:
        cmd(ws, "Emulation.setDeviceMetricsOverride", {"width": w, "height": h, "deviceScaleFactor": 1, "mobile": True})
        time.sleep(0.5)
        assert_true(ev(ws, "document.documentElement.scrollWidth <= document.documentElement.clientWidth") is True,
                    "无水平溢出(%s)" % name)
    # 2) 手机视口:抽屉可达性——顶栏汉堡可见 → 点开抽屉 → 点「组合订阅」→ 页面导航且抽屉关闭
    cmd(ws, "Emulation.setDeviceMetricsOverride", {"width": 390, "height": 844, "deviceScaleFactor": 1, "mobile": True})
    time.sleep(1)
    # 汉堡按钮在 DOM 恒存在(桌面 CSS 隐藏顶栏),故断言加 display 检查保证「可见」语义
    assert_true(ev(ws, "!!document.querySelector('.topbar-menu') && getComputedStyle(document.querySelector('.topbar')).display !== 'none'") is True,
                "手机视口顶栏汉堡可见")
    ev(ws, "document.querySelector('.topbar-menu').click()")
    time.sleep(0.4)
    assert_true(ev(ws, "document.querySelector('.sidebar').classList.contains('open')"), "抽屉打开")
    # 组合订阅是 .sidebar 内 NavLeaf 按钮;移动端分组默认折叠,须先展开「订阅管理」分组头
    # (分组头点击只改 open_groups,不关抽屉);选中导航项时 go() 同步关闭抽屉。
    ev(ws, "Array.from(document.querySelectorAll('.sidebar button')).find(b=>b.textContent.includes('订阅管理')).click()")
    time.sleep(0.3)
    assert_true(ev(ws, "Array.from(document.querySelectorAll('.sidebar button')).some(b=>b.textContent.includes('组合订阅'))"), "展开分组后组合订阅可见")
    ev(ws, "Array.from(document.querySelectorAll('.sidebar button')).find(b=>b.textContent.includes('组合订阅')).click()")
    time.sleep(0.4)
    assert_true(ev(ws, "!document.querySelector('.sidebar').classList.contains('open')"), "选中项后抽屉关闭")
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('.sidebar button')).find(b=>b.textContent.includes('组合订阅')).classList.contains('active')", timeout=10),
                "组合订阅页导航激活(抽屉内导航可达)")
    # 3) 跨断点 resize:桌面侧栏常驻(sticky) → 手机顶栏出现(显示而非仅 DOM 存在)
    cmd(ws, "Emulation.setDeviceMetricsOverride", {"width": 1280, "height": 800, "deviceScaleFactor": 1, "mobile": False})
    time.sleep(0.8)
    assert_true(ev(ws, "getComputedStyle(document.querySelector('.sidebar')).position === 'sticky'"), "桌面侧栏常驻(sticky)")
    cmd(ws, "Emulation.setDeviceMetricsOverride", {"width": 390, "height": 844, "deviceScaleFactor": 1, "mobile": True})
    time.sleep(0.8)
    assert_true(ev(ws, "getComputedStyle(document.querySelector('.topbar')).display !== 'none'") is True, "resize 后顶栏出现")

def scenario_theme_switch(ws):
    """主题切换：三态分段按钮存在 → 切深色 → html[data-theme]='dark' 生效 + localStorage 写入
    → 刷新后保持深色 → 切回 system 恢复跟随。"""
    login(ws)
    assert_true(wait_until(ws, "!!document.querySelector('.theme-switcher')"), "主题切换器出现")
    # 默认 system（新库/未设置过）
    assert_true(ev(ws, "document.documentElement.dataset.theme === 'system'"), "默认 system 主题")
    # 切深色
    ev(ws, "Array.from(document.querySelectorAll('.theme-switcher .seg')).find(b=>b.title==='深色').click()")
    assert_true(wait_until(ws, "document.documentElement.dataset.theme === 'dark'"), "data-theme 切为 dark")
    assert_true(ev(ws, "localStorage.getItem('submerge_theme') === 'dark'"), "localStorage 写入 dark")
    # 刷新后保持
    cmd(ws, "Page.reload")
    time.sleep(2.5)
    assert_true(wait_until(ws, "document.documentElement.dataset.theme === 'dark'", timeout=15), "刷新后保持 dark")
    # 切回 system（恢复跟随，不污染共享环境）
    ev(ws, "Array.from(document.querySelectorAll('.theme-switcher .seg')).find(b=>b.title==='跟随系统').click()")
    assert_true(wait_until(ws, "document.documentElement.dataset.theme === 'system'"), "切回 system")
    assert_true(ev(ws, "localStorage.getItem('submerge_theme') === 'system'"), "localStorage 写入 system")

def main():
    scenario = sys.argv[1] if len(sys.argv) > 1 else "nav_preload"
    ws = connect()
    scenarios = {"nav_preload": scenario_nav_preload, "sources_crud": scenario_sources_crud,
                 "combineds": scenario_combineds, "preview": scenario_preview,
                 "preview_failure": scenario_preview_failure,
                 "config_password": scenario_config_password,
                 "responsive": scenario_responsive,
                 "theme_switch": scenario_theme_switch}
    scenarios[scenario](ws)
    print("== %s: ALL PASS ==" % scenario)

if __name__ == "__main__":
    main()
