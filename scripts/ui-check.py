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
        # 超时/断连返回 None(假值):wait_until/assert_true 会自然 FAIL,不得返回真值。
        return None

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
    # 注意:querySelector 未命中返回 null,null !== undefined 恒真,必须 !! 包裹。
    return ev(ws, "!!(Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('%s'))?.querySelector('.spinner'))" % label)

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

def cleanup_combined(name):
    """经 API 删除同名组合(幂等)。combined_subs.name 有 UNIQUE 约束,
    场景重跑前须先清掉同名旧数据,否则保存会 400 失败。"""
    import urllib.request as u
    req = u.Request(URL + "/admin/combineds", headers={"Authorization": "Bearer test-token"})
    with u.urlopen(req, timeout=5) as r:
        items = json.loads(r.read())
    for it in items:
        if it.get("name") == name:
            u.urlopen(u.Request(URL + "/admin/combineds/%d" % it["id"], method="DELETE",
                                headers={"Authorization": "Bearer test-token"}), timeout=5)

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
    """首次切换:旧页保持 + 菜单项转圈 → 就绪后切换;已加载页回访秒开。"""
    seed_sources(ws, 1)
    cmd(ws, "Page.addScriptToEvaluateOnNewDocument", {"source": OBSERVER_JS})
    login(ws)
    assert_true(wait_until(ws, "window.__ui.saw_spinner===true", timeout=15), "初始概览加载中菜单项转圈")
    assert_true(wait_until(ws, "window.__ui.saw_loading===true", timeout=3), "初始加载内容区显示全页 loading")
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('概览')).classList.contains('active')"), "概览就绪后激活")
    # 回访秒开:订阅源单元已随概览预载 → 点击立即切换
    click_nav(ws, "订阅源")
    assert_true(nav_active(ws, "订阅源"), "已缓存单元切换秒开(无转圈)")
    assert_true(not nav_loading(ws, "订阅源"), "秒开路径无转圈")
    # 概览回访:数据缓存,秒开
    click_nav(ws, "概览")
    assert_true(nav_active(ws, "概览"), "概览回访秒开")
    assert_true(not nav_loading(ws, "概览"), "概览回访无转圈")

def scenario_sources_crud(ws):
    """订阅源页添加源 → 切概览 → 统计同步(缓存回写)。"""
    login(ws)
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('概览')).classList.contains('active')"), "概览就绪")
    # 记录添加前的源总数:DB 会跨场景累积(seed/重复运行),断言用 N+1 而非写死。
    assert_true(wait_until(ws, "!!document.querySelector('.stat-value')"), "概览统计已渲染")
    n0 = ev(ws, "parseInt(document.querySelector('.stat-value')?.textContent ?? '0', 10)")
    assert_true(isinstance(n0, int), "读取到添加前源总数")
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
    assert_true(ev(ws, "document.querySelector('.stat-value')?.textContent === '%d'" % (n0 + 1)), "概览源总数 +1(缓存回写)")

def scenario_preview_filter(ws):
    """预览页:全部源视图来自缓存;过滤下拉走本地拉取。"""
    import urllib.request as u
    # 全部源节点数随 DB 累积变化(seed 的 2 个 single 源各产出 1 节点),以 API 实时节点数为基准。
    req = u.Request(URL + "/admin/preview", headers={"Authorization": "Bearer test-token"})
    with u.urlopen(req, timeout=5) as r:
        n0 = len(json.loads(r.read())["nodes"])
    # 确保 c-test 组合存在(combineds 场景会创建;缺失时经 API 补建,成员取首个源)
    req = u.Request(URL + "/admin/combineds", headers={"Authorization": "Bearer test-token"})
    with u.urlopen(req, timeout=5) as r:
        combos = json.loads(r.read())
    if not any(c.get("name") == "c-test" for c in combos):
        req = u.Request(URL + "/admin/sources", headers={"Authorization": "Bearer test-token"})
        with u.urlopen(req, timeout=5) as r:
            first_id = json.loads(r.read())[0]["id"]
        req = u.Request(URL + "/admin/combineds", method="POST",
                        data=json.dumps({"name": "c-test", "source_ids": [first_id]}).encode(),
                        headers={"Authorization": "Bearer test-token", "Content-Type": "application/json"})
        u.urlopen(req, timeout=5)
    seed_sources(ws, 2)
    login(ws)
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('概览')).classList.contains('active')"), "概览就绪")
    click_nav(ws, "预览")
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('预览')).classList.contains('active')"), "预览就绪")
    assert_true(wait_until(ws, "document.querySelectorAll('.table-wrap tbody tr').length === %d" % (n0 + 2)), "全部源视图 %d 个节点" % (n0 + 2))
    # 切过滤下拉(选项来自 combineds 缓存单元,先等它出现)
    assert_true(wait_until(ws, "!!Array.from(document.querySelector('.preview-filter')?.options ?? []).find(o=>o.textContent==='c-test')"), "下拉出现 c-test 选项")
    ev(ws, "(()=>{const sel=document.querySelector('.preview-filter');const t=Array.from(sel.options).find(o=>o.textContent==='c-test');sel.value=t.value;sel.dispatchEvent(new Event('change',{bubbles:true}));})()")
    time.sleep(0.8)
    assert_true(ev(ws, "document.querySelectorAll('.table-wrap tbody tr').length === 1"), "过滤视图只显示该组合成员")

def find_server():
    """定位 :18080 的 server 进程(Linux /proc 扫描,按 PORT=18080 + test-token 过滤,避免误伤其他 server)。"""
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
        if "PORT=18080" not in env or "SUB_MERGE_ADMIN_TOKEN=test-token" not in env:
            continue
        try:
            exe = os.readlink("/proc/%s/exe" % pid)
            cwd = os.readlink("/proc/%s/cwd" % pid)
        except OSError:
            continue
        hits.append((pid, exe, cwd, env))
    return hits

def scenario_refresh_failure(ws):
    """刷新失败:停 server → 点刷新 → 旧数据行仍在 + 错误文本出现 → 重启恢复(错误清除)。"""
    import os, signal, subprocess
    hits = find_server()
    assert_true(len(hits) > 0, "找到 :18080 server 进程")
    pid, exe, cwd, env = hits[0]
    login(ws)
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('概览')).classList.contains('active')"), "概览就绪")
    assert_true(wait_until(ws, "!!document.querySelector('.stat-value')"), "概览统计已渲染")
    stats0 = ev(ws, "Array.from(document.querySelectorAll('.stat-value')).map(e=>e.textContent).join(',')")
    rows0 = ev(ws, "document.querySelectorAll('.summary-row').length")
    assert_true(isinstance(rows0, int) and rows0 > 0, "订阅源摘要行已渲染(%d)" % (rows0 if isinstance(rows0, int) else -1))
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
    # 点刷新:旧数据应保留 + 错误文本出现(修复前 Error 清空 data,统计归零/摘要消失)
    ev(ws, "Array.from(document.querySelectorAll('button')).find(b=>b.textContent.includes('刷新')).click()")
    assert_true(wait_until(ws, "!!document.querySelector('.error-text')", timeout=10), "刷新失败后错误文本出现")
    assert_true(ev(ws, "Array.from(document.querySelectorAll('.stat-value')).map(e=>e.textContent).join(',')") == stats0, "刷新失败后统计值不变(旧数据保留)")
    assert_true(ev(ws, "document.querySelectorAll('.summary-row').length") == rows0, "刷新失败后摘要行仍在(旧数据保留)")
    # 重启 server(按原进程的 exe/cwd/env 重建,detached 脱离本会话)
    env = dict(kv.split("=", 1) for kv in env if "=" in kv)
    log = open("/tmp/submerge-server-restart.log", "ab")
    subprocess.Popen([exe], cwd=cwd, env=env, stdout=log, stderr=log, start_new_session=True)
    alive = False
    for _ in range(40):
        try:
            with urllib.request.urlopen(URL + "/healthz", timeout=1) as r:
                if r.status == 200:
                    alive = True
                    break
        except Exception:
            time.sleep(0.5)
    assert_true(alive, "server 重启成功(/healthz 200)")
    # 恢复:再次刷新,错误消失
    ev(ws, "Array.from(document.querySelectorAll('button')).find(b=>b.textContent.includes('刷新')).click()")
    assert_true(wait_until(ws, "!document.querySelector('.error-text')", timeout=10), "重启后刷新恢复,错误消失")

def restore_admin_token():
    """轮换后把服务端 admin token 恢复为 test-token 并重启 server。

    轮换 API 只生成随机新 token（DB 落库 + 内存态更新），无法回写指定值；
    场景末尾直写 DB 后重启（ensure_tokens 启动时重读 DB），后续场景才可继续用 test-token。"""
    import os, signal, subprocess
    import sqlite3
    hits = find_server()
    assert_true(len(hits) > 0, "找到 :18080 server 进程")
    pid, exe, cwd, env = hits[0]
    envmap = dict(kv.split("=", 1) for kv in env if "=" in kv)
    db = envmap.get("DATABASE_PATH") or os.path.join(cwd, "submerge.db")
    conn = sqlite3.connect(db)
    conn.execute("UPDATE settings SET value='test-token' WHERE key='admin_token'")
    conn.commit()
    conn.close()
    # 重启使内存态生效（admin_token 在 Arc<RwLock>，只有启动时会从 DB 重读）
    os.kill(int(pid), signal.SIGTERM)
    for _ in range(20):
        if not os.path.exists("/proc/" + pid):
            break
        time.sleep(0.5)
    log = open("/tmp/submerge-server-restore.log", "ab")
    subprocess.Popen([exe], cwd=cwd, env=envmap, stdout=log, stderr=log, start_new_session=True)
    alive = False
    for _ in range(40):
        try:
            with urllib.request.urlopen(URL + "/healthz", timeout=1) as r:
                if r.status == 200:
                    alive = True
                    break
        except Exception:
            time.sleep(0.5)
    assert_true(alive, "server 重启成功(/healthz 200)，admin token 已恢复 test-token")

def scenario_config_rotate(ws):
    """配置页:token 显示来自缓存;轮换后回写缓存 + 会话同步。"""
    login(ws)
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('概览')).classList.contains('active')"), "概览就绪")
    click_nav(ws, "配置")
    assert_true(wait_until(ws, "Array.from(document.querySelectorAll('nav button')).find(b=>b.textContent.includes('配置')).classList.contains('active')"), "配置就绪")
    # 默认掩码显示是固定字符串(mask_token 恒为 ••••••••)，先点「显示」取明文，轮换后才有文本变化可断言
    assert_true(wait_until(ws, "!!document.querySelector('.token-value')"), "token 卡片已渲染(缓存读取)")
    ev(ws, "Array.from(document.querySelectorAll('button')).find(b=>b.textContent.includes('显示')).click()")
    time.sleep(0.3)
    old = ev(ws, "document.querySelector('.token-value')?.textContent")
    assert_true(bool(old), "读取到明文 token")
    ev(ws, "Array.from(document.querySelectorAll('button')).find(b=>b.textContent.includes('轮换')).click()")
    time.sleep(0.5)
    ev(ws, "Array.from(document.querySelectorAll('.modal-actions button')).find(b=>b.textContent.includes('轮换')).click()")
    assert_true(wait_until(ws, "document.querySelector('.token-value')?.textContent !== '%s'" % old), "轮换后 token 显示已更新(缓存回写)")
    # 服务端已轮换:旧 token 立即失效(API 直查 401)
    import urllib.request as u
    rejected = False
    try:
        req = u.Request(URL + "/admin/config", headers={"Authorization": "Bearer " + old})
        u.urlopen(req, timeout=5)
    except Exception as e:
        rejected = getattr(e, "code", None) == 401
    assert_true(rejected, "旧 token 已失效(API 401)")
    # 恢复 test-token,使后续场景不受轮换影响
    restore_admin_token()

def scenario_combineds(ws):
    """组合订阅:新建 → 列表出现;保存后缓存 refresh 回写。"""
    cleanup_combined("c-test")
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

def main():
    scenario = sys.argv[1] if len(sys.argv) > 1 else "nav_preload"
    ws = connect()
    scenarios = {"nav_preload": scenario_nav_preload, "sources_crud": scenario_sources_crud,
                 "combineds": scenario_combineds, "preview_filter": scenario_preview_filter,
                 "refresh_failure": scenario_refresh_failure,
                 "config_rotate": scenario_config_rotate}
    scenarios[scenario](ws)
    print("== %s: ALL PASS ==" % scenario)

if __name__ == "__main__":
    main()
