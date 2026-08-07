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
    """页面登录:API 层拿真实会话 token → 写入 localStorage(submerge_admin_session) → 刷新。"""
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

def preview_rows(ws):
    """预览区节点表格行数（页面另有「订阅源列表」表格，同用 .table-wrap tbody tr）。

    按卡片标题「预览」定位预览卡片再数行：比 :last-of-type 稳（预览卡片后面的
    弹窗/后续卡片不影响），也比 .preview-toolbar ~ .table-wrap 语义稳（组件
    内部结构变化不敏感）。卡片缺失时返回 null，让调用方断言自然失败。"""
    return ev(ws, "(()=>{const c=Array.from(document.querySelectorAll('.card')).find(c=>c.querySelector('h2.card-title')?.textContent==='预览');return c?c.querySelectorAll('.table-wrap tbody tr').length:null})()")

def click_refresh(ws):
    """点预览区「刷新预览」按钮;页面无该按钮时回退到任一「刷新」按钮。

    不用 `x?.click() ?? y.click()` 形式:click() 返回 undefined(nullish),
    ?? 右侧仍会执行导致双重点击。"""
    ev(ws, "(()=>{const b=Array.from(document.querySelectorAll('button')).find(b=>b.textContent.includes('刷新预览'))||Array.from(document.querySelectorAll('button')).find(b=>b.textContent.includes('刷新'));if(b)b.click();})()")

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
    # 回访秒开:切远程订阅(同 sources 单元缓存) → 立即切换
    click_nav(ws, "远程订阅")
    assert_true(nav_active(ws, "远程订阅"), "同单元切换秒开")
    assert_true(not nav_loading(ws, "远程订阅"), "秒开路径无转圈")
    # 慢路径(点按切换):注入 4s 网络延迟 → 点「配置」(首个请求,无缓存) →
    # 加载窗口内旧页保持可见 + 菜单项转圈 + 未提前切换 → 就绪后切换完成。
    cmd(ws, "Network.enable")
    cmd(ws, "Network.emulateNetworkConditions",
        {"offline": False, "latency": 4000, "downloadThroughput": -1, "uploadThroughput": -1})
    click_nav(ws, "配置")
    time.sleep(1.0)
    # 旧页保持:nav 文案恒在 body 内(远程订阅菜单项常在),不能只查 body 文本——
    # 断言激活态 + 页面内容(订阅源列表徽章,配置页无此结构)。
    assert_true(ev(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('远程订阅')).classList.contains('active')"), "慢加载期间旧页(远程订阅)保持激活")
    assert_true(ev(ws, "!!document.querySelector('.card h2 + .badge')"), "慢加载期间旧页内容(订阅源列表徽章)保持可见")
    assert_true(nav_loading(ws, "配置"), "慢加载期间目标菜单项转圈")
    assert_true(not nav_active(ws, "配置"), "慢加载期间未提前切换")
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('配置')).classList.contains('active')", timeout=15), "就绪后切换完成")
    cmd(ws, "Network.emulateNetworkConditions",
        {"offline": False, "latency": 0, "downloadThroughput": -1, "uploadThroughput": -1})

def scenario_first_load_failure(ws):
    """首次加载失败(预览区请求):预览区错误文本 + 页面仍切换(单元失败不再阻塞——本地订阅仅需 sources 单元)。

    用 CDP 请求拦截让 /admin/preview 首次请求即失败。种源不可行:单条节点解析失败的
    URI / 指向死端口的 remote 源,服务端只返回 200 + 源错误列表(单元仍 Ready),
    不会进入 CacheStatus::Error —— 拦截请求才能真实覆盖错误可见路径。"""
    cmd(ws, "Network.enable")
    cmd(ws, "Network.setBlockedURLs", {"urls": ["*admin/preview*"]})
    cmd(ws, "Page.enable"); cmd(ws, "Runtime.enable")
    time.sleep(2)
    for _ in range(20):
        if ev(ws, "document.readyState") == "complete":
            break
        time.sleep(0.5)
    # 登录走 API(setup-status/login 不在拦截范围),再注入会话刷新页面
    login(ws)
    # 初始 tab=0(本地订阅)仅需 sources 单元:不被 preview 拦截阻塞,页面照常切换。
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('本地订阅')).classList.contains('active')", timeout=15), "初始页切换(本地订阅激活)")
    # 预览区挂载自动拉取被拦截失败 → 预览区错误文本
    assert_true(wait_until(ws, "!!document.querySelector('.error-text')", timeout=10), "预览区出现错误文本(拦截失败)")
    # 解除拦截,点预览区刷新 → 恢复
    cmd(ws, "Network.setBlockedURLs", {"urls": []})
    click_refresh(ws)
    assert_true(wait_until(ws, "!document.querySelector('.error-text')", timeout=15), "解除后刷新恢复(错误消失)")

def scenario_sources_crud(ws):
    """本地订阅页添加源 → 计数徽章 +1(原概览统计断言迁移)。"""
    login(ws)
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('本地订阅')).classList.contains('active')"), "本地订阅就绪")
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

def scenario_preview_filter(ws):
    """组合订阅页:预览下拉切换组合(节点表渲染)。"""
    import urllib.request as u
    ensure_session()
    # 确保 c-test 组合存在(combineds 场景会创建;缺失时经 API 补建,成员取首个源)
    req = u.Request(URL + "/admin/combineds", headers={"Authorization": "Bearer " + SESSION_TOKEN})
    with u.urlopen(req, timeout=5) as r:
        combos = json.loads(r.read())
    if not any(c.get("name") == "c-test" for c in combos):
        req = u.Request(URL + "/admin/sources", headers={"Authorization": "Bearer " + SESSION_TOKEN})
        with u.urlopen(req, timeout=5) as r:
            sources = json.loads(r.read())
        if not sources:
            # 空 DB：先经 API 种一个 single 源（与 seed_sources 相同节点形态），再取 first_id
            u.urlopen(u.Request(URL + "/admin/sources", method="POST",
                                data=json.dumps({"name": "c-seed",
                                                 "url": "vless://e99a8e5a-6b2b-4a1d-9c5f-1a2b3c4d5e6f@1.2.3.4:443#c-seed",
                                                 "kind": "single"}).encode(),
                                headers={"Authorization": "Bearer " + SESSION_TOKEN,
                                         "Content-Type": "application/json"}), timeout=5)
            req = u.Request(URL + "/admin/sources", headers={"Authorization": "Bearer " + SESSION_TOKEN})
            with u.urlopen(req, timeout=5) as r:
                sources = json.loads(r.read())
        first_id = sources[0]["id"]
        req = u.Request(URL + "/admin/combineds", method="POST",
                        data=json.dumps({"name": "c-test", "source_ids": [first_id]}).encode(),
                        headers={"Authorization": "Bearer " + SESSION_TOKEN, "Content-Type": "application/json"})
        u.urlopen(req, timeout=5)
    seed_sources(ws, 2)
    login(ws)
    click_nav(ws, "组合订阅")
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('组合订阅')).classList.contains('active')"), "组合订阅就绪")
    # 预览下拉出现 c-test 选项 → 切换 → 节点表渲染
    assert_true(wait_until(ws, "!!Array.from(document.querySelectorAll('.preview-filter option')).find(o=>o.textContent==='c-test')"), "预览下拉出现 c-test")
    ev(ws, "(()=>{const sel=document.querySelector('.preview-filter');const t=Array.from(sel.options).find(o=>o.textContent==='c-test');sel.value=t.value;sel.dispatchEvent(new Event('change',{bubbles:true}));})()")
    # c-test 恒 1 成员 1 节点 → 切换后表格行数确定性 === 1。重拉期间旧 data 保留
    # （行数可能是旧值），wait_until 轮询直到重拉完成行数收敛为 1。
    assert_true(wait_until(ws, "document.querySelectorAll('.table-wrap tbody tr').length === 1", timeout=10), "过滤视图只显示该组合成员")

def find_server():
    """定位 :18080 的 server 进程(Linux /proc 扫描,按 PORT=18080 过滤,避免误伤其他 server)。

    鉴权已改为用户名+密码,不再需要预设 token 环境变量,故只按 PORT 过滤。"""
    import os
    hits = []
    for pid in os.listdir("/proc"):
        if not pid.isdigit():
            continue
        try:
            with open("/proc/%s/environ" % pid, "rb") as f:
                env = f.read().decode("utf-8", "replace").split("\0")
        except OSError:
            continue
        if "PORT=18080" not in env:
            continue
        try:
            exe = os.readlink("/proc/%s/exe" % pid)
            cwd = os.readlink("/proc/%s/cwd" % pid)
        except OSError:
            continue
        # 运行期间二进制被重新构建替换时,/proc/PID/exe 会带 " (deleted)" 后缀,
        # 直接重启会 FileNotFoundError——剥掉后缀用当前路径上的新二进制重启。
        if exe.endswith(" (deleted)"):
            exe = exe[: -len(" (deleted)")]
        hits.append((pid, exe, cwd, env))
    return hits

def wait_healthz(timeout=20):
    """等待 :18080 /healthz 返回 200。"""
    for _ in range(int(timeout / 0.5)):
        try:
            with urllib.request.urlopen(URL + "/healthz", timeout=1) as r:
                if r.status == 200:
                    return True
        except Exception:
            pass
        time.sleep(0.5)
    return False

def restart_server(pid, exe, cwd, envmap, logpath):
    """确保旧进程退出(等退出,超时 SIGKILL)后按原 exe/cwd/env 重启 server。

    pid 已不存在时直接启动(幂等)。调用方随后用 wait_healthz 断言存活。"""
    import os, signal, subprocess
    if os.path.exists("/proc/" + pid):
        for _ in range(20):
            if not os.path.exists("/proc/" + pid):
                break
            time.sleep(0.5)
    if os.path.exists("/proc/" + pid):
        try:
            os.kill(int(pid), signal.SIGKILL)
        except OSError:
            pass
        time.sleep(0.5)
    log = open(logpath, "ab")
    subprocess.Popen([exe], cwd=cwd, env=envmap, stdout=log, stderr=log, start_new_session=True)

def scenario_refresh_failure(ws):
    """刷新失败:停 server → 预览区「刷新预览」→ 错误文本出现 + 旧数据(节点行)保留 → 重启恢复。"""
    import os, signal
    hits = find_server()
    assert_true(len(hits) > 0, "找到 :18080 server 进程")
    pid, exe, cwd, env = hits[0]
    envmap = dict(kv.split("=", 1) for kv in env if "=" in kv)
    # 种 1 个源保证预览区有节点可渲染(行数断言需要旧数据)
    seed_sources(ws, 1)
    login(ws)
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('本地订阅')).classList.contains('active')"), "本地订阅就绪")
    # 预览区挂载自动拉取:等「共 N 个节点」徽章渲染(数据就绪信号),记录预览表格行数
    assert_true(wait_until(ws, "!!document.querySelector('.preview-toolbar .badge')"), "预览区已加载(节点徽章渲染)")
    rows0 = preview_rows(ws)
    assert_true(isinstance(rows0, int) and rows0 > 0, "预览表格行已渲染(%d)" % (rows0 if isinstance(rows0, int) else -1))
    try:
        # 停 server
        for p, _, _, _ in hits:
            try:
                os.kill(int(p), signal.SIGTERM)
            except OSError:
                pass
        for _ in range(20):
            if not os.path.exists("/proc/" + pid):
                break
            time.sleep(0.5)
        assert_true(not os.path.exists("/proc/" + pid), "server 已停止")
        # 点预览区刷新:旧数据应保留 + 错误文本出现(修复前 Error 清空 data,表格行消失)
        click_refresh(ws)
        assert_true(wait_until(ws, "!!document.querySelector('.error-text')", timeout=10), "刷新失败后错误文本出现")
        assert_true(preview_rows(ws) == rows0, "刷新失败后预览表格行数不变(旧数据保留)")
    finally:
        # 恢复:断言失败也不留死 server(死 server 会让后续场景全部 401/挂起)
        restart_server(pid, exe, cwd, envmap, "/tmp/submerge-server-restart.log")
    assert_true(wait_healthz(), "server 重启成功(/healthz 200)")
    # 恢复:再次刷新,错误消失
    click_refresh(ws)
    assert_true(wait_until(ws, "!document.querySelector('.error-text')", timeout=10), "重启后刷新恢复,错误消失")

def scenario_config_password(ws):
    """配置页:账号卡片渲染用户名(缓存读取);改密后全部会话失效被踢回登录页;新密码重新登录。

    收尾(finally)用 API 把密码改回 ui-pass-12345 并刷新 SESSION_TOKEN,
    使后续场景不受影响(改密后任一步断言失败也执行恢复)。"""
    global SESSION_TOKEN
    NEW_PASS = "ui-pass-67890"
    login(ws)
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('本地订阅')).classList.contains('active')"), "本地订阅就绪")
    click_nav(ws, "配置")
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('配置')).classList.contains('active')"), "配置就绪")
    # 账号卡片:用户名来自 DataStore 缓存(GET /admin/config),渲染为 .token-row .token-value
    assert_true(wait_until(ws, "document.querySelector('.card .token-value')?.textContent === '%s'" % ADMIN_USER), "账号卡片渲染用户名 ui")
    # 三个密码输入按渲染顺序:当前密码/新密码/确认新密码
    ev(ws, "(()=>{const ins=document.querySelectorAll('.card input');ins[0].value='%s';ins[0].dispatchEvent(new Event('input',{bubbles:true}));ins[1].value='%s';ins[1].dispatchEvent(new Event('input',{bubbles:true}));ins[2].value='%s';ins[2].dispatchEvent(new Event('input',{bubbles:true}));})()" % (ADMIN_PASS, NEW_PASS, NEW_PASS))
    ev(ws, "Array.from(document.querySelectorAll('button')).find(b=>b.textContent.includes('修改密码')).click()")
    try:
        # 改密成功后服务端使全部会话失效,本地清除会话 → 回登录页
        assert_true(wait_until(ws, "!!document.querySelector('.login-card')", timeout=10), "改密后被踢回登录页")
        # 登录页探测 setup 状态后出现登录表单:用新密码走页面表单重新登录(全链路)
        assert_true(wait_until(ws, "!!Array.from(document.querySelectorAll('.login-card button')).find(b=>b.textContent.includes('登录'))", timeout=10), "登录页就绪(显示登录表单)")
        ev(ws, "(()=>{const ins=document.querySelectorAll('.login-card input');ins[0].value='%s';ins[0].dispatchEvent(new Event('input',{bubbles:true}));ins[1].value='%s';ins[1].dispatchEvent(new Event('input',{bubbles:true}));})()" % (ADMIN_USER, NEW_PASS))
        ev(ws, "Array.from(document.querySelectorAll('.login-card button')).find(b=>b.textContent.includes('登录')).click()")
        assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('本地订阅')).classList.contains('active')", timeout=15), "新密码登录成功(进入本地订阅)")
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
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('本地订阅')).classList.contains('active')"), "本地订阅就绪")
    click_nav(ws, "组合订阅")
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('组合订阅')).classList.contains('active')"), "组合订阅就绪")
    ev(ws, "Array.from(document.querySelectorAll('button')).find(b=>b.textContent.includes('新建组合')).click()")
    time.sleep(0.5)
    ev(ws, "(()=>{const el=document.querySelector('.modal input');const s=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set;s.call(el,'c-test');el.dispatchEvent(new Event('input',{bubbles:true}));})()")
    ev(ws, "document.querySelector('.member-row input').click()")
    time.sleep(0.3)
    ev(ws, "Array.from(document.querySelectorAll('.modal-actions button')).find(b=>b.textContent.includes('保存')).click()")
    assert_true(wait_until(ws, "document.body.innerText.includes('c-test')"), "保存后列表出现 c-test")

def main():
    scenario = sys.argv[1] if len(sys.argv) > 1 else "nav_preload"
    ws = connect()
    scenarios = {"nav_preload": scenario_nav_preload, "sources_crud": scenario_sources_crud,
                 "combineds": scenario_combineds, "preview_filter": scenario_preview_filter,
                 "refresh_failure": scenario_refresh_failure,
                 "config_password": scenario_config_password,
                 "first_load_failure": scenario_first_load_failure}
    scenarios[scenario](ws)
    print("== %s: ALL PASS ==" % scenario)

if __name__ == "__main__":
    main()
