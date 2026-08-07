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

def main():
    scenario = sys.argv[1] if len(sys.argv) > 1 else "nav_preload"
    ws = connect()
    scenarios = {"nav_preload": scenario_nav_preload}
    scenarios[scenario](ws)
    print("== %s: ALL PASS ==" % scenario)

if __name__ == "__main__":
    main()
