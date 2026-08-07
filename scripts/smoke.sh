#!/usr/bin/env bash
# sub-merge 端到端冒烟测试
#
# 验证链路：
#   0. cargo build -p server（生成可执行二进制）
#   1. dx build --web --debug-symbols false 构建前端 → dist/（不带标志时 wasm-opt 会 SIGABRT，见 CLAUDE.md 坑清单）
#   2. server 以 WEB_DIST 指向 dist 启动（临时 DB、随机端口）
#   3. curl 根路径（health）→ "sub-merge is running"
#   4. curl 静态资源 index.html / wasm js / wasm binary → 200
#   5. 首次引导创建管理员 + login 拿会话 → /admin/config（Bearer）→ 返回用户名
#   6. 加源 + 创建组合订阅勾选成员 → /subscribe/merged（无 token）输出节点
#   7. 组合订阅名不匹配 → 404
#   8. 未知 /admin/* → JSON 404（不回退 SPA）
#   9. SPA 回退
#
# 用法：bash scripts/smoke.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export PATH="$HOME/.cargo/bin:$PATH"

SERVER_PORT="${SERVER_PORT:-18080}"
FIXTURE_PORT="${FIXTURE_PORT:-18081}"
TMP_DIR="$(mktemp -d)"
SERVER_PID=""
FIXTURE_PID=""
cleanup() {
  [[ -n "$SERVER_PID" ]] && kill "$SERVER_PID" 2>/dev/null || true
  [[ -n "$FIXTURE_PID" ]] && kill "$FIXTURE_PID" 2>/dev/null || true
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

step() { printf '\n=== %s ===\n' "$1"; }
fail() { printf 'FAIL: %s\n' "$1" >&2; exit 1; }

# ---- 0. 构建 server 二进制 ----
step "0/9 cargo build -p server"
cargo build -p server >/dev/null 2>&1 || fail "cargo build -p server 失败"
SERVER_BIN="$ROOT/target/debug/server"
[[ -x "$SERVER_BIN" ]] || fail "server 二进制不存在：$SERVER_BIN"

# ---- 1. 构建前端 ----
step "1/9 dx build --web --release --debug-symbols false"
(
  cd "$ROOT/crates/server/web"
  dx build --web --release --debug-symbols false
) || fail "dx build --web --release 失败"

WEB_DIST="$ROOT/crates/server/web/dist"
[[ -f "$WEB_DIST/index.html" ]] || fail "dist/index.html 不存在（WEB_DIST=$WEB_DIST）"
printf 'dist 就绪: %s\n' "$(readlink -f "$WEB_DIST")"

# ---- 2. fixture 订阅源（一个合法 vless 节点）----
step "2/9 起 fixture 订阅源服务"
cat > "$TMP_DIR/sub.txt" <<'EOF'
vless://e99a8e5a-6b2b-4a1d-9c5f-1a2b3c4d5e6f@1.2.3.4:443#fixture-node
EOF
python3 -m http.server "$FIXTURE_PORT" --bind 127.0.0.1 --directory "$TMP_DIR" >/dev/null 2>&1 &
FIXTURE_PID=$!
for i in $(seq 1 20); do
  if curl -sf "http://127.0.0.1:$FIXTURE_PORT/sub.txt" >/dev/null 2>&1; then break; fi
  sleep 0.25
  [[ $i -eq 20 ]] && fail "fixture 服务未就绪"
done
printf 'fixture 就绪（port %s）\n' "$FIXTURE_PORT"

# ---- 3. 启动 server ----
step "3/9 启动 server（WEB_DIST=$WEB_DIST）"
WEB_DIST="$WEB_DIST" \
DATABASE_PATH="$TMP_DIR/submerge-smoke.db" \
PORT="$SERVER_PORT" \
"$SERVER_BIN" >"$TMP_DIR/server.log" 2>&1 &
SERVER_PID=$!

for i in $(seq 1 40); do
  if curl -sf "http://127.0.0.1:$SERVER_PORT/" >/dev/null 2>&1; then break; fi
  sleep 0.25
  if ! kill -0 "$SERVER_PID" 2>/dev/null; then
    cat "$TMP_DIR/server.log" >&2
    fail "server 启动失败"
  fi
  [[ $i -eq 40 ]] && { cat "$TMP_DIR/server.log" >&2; fail "server 启动超时"; }
done
printf 'server 就绪\n'

# ---- 4. 健康检查 + 根路径 SPA + 静态资源 ----
step "4/9 健康检查 + 根路径 + 静态资源"
health="$(curl -sf "http://127.0.0.1:$SERVER_PORT/healthz")"
grep -q "sub-merge is running" <<<"$health" || fail "健康检查：$health"
printf 'GET /healthz   → OK (%s)\n' "$(head -c 40 <<<"$health")"

# 根路径必须返回 SPA（浏览器直接打开 / 即见管理界面），而非健康检查文本
root_spa="$(curl -sf "http://127.0.0.1:$SERVER_PORT/")"
grep -q "sub-merge" <<<"$root_spa" || fail "GET / 未返回 SPA index.html"
printf 'GET /          → SPA index.html\n'

curl -sf "http://127.0.0.1:$SERVER_PORT/index.html" -o "$TMP_DIR/spa-index.html"
grep -q "sub-merge" "$TMP_DIR/spa-index.html" || fail "/index.html 未包含 SPA 标题"
printf 'GET /index.html → 200 OK\n'

# release 产物带内容 hash：入口脚本与 wasm 路径从 index.html / JS 动态解析
spa_js="$(grep -o 'src="[^"]*\.js"' "$TMP_DIR/spa-index.html" | head -1 | sed 's/src="//;s/"//' | sed 's|^/\./|/|')"
[[ -n "$spa_js" ]] || fail "index.html 中未找到入口脚本"
curl -sf "http://127.0.0.1:$SERVER_PORT$spa_js" -o "$TMP_DIR/spa-wasm.js"
grep -q "wasm" "$TMP_DIR/spa-wasm.js" || fail "$spa_js 内容异常"
printf 'GET %s → 200 OK\n' "$spa_js"

spa_wasm="$(grep -o '/\./assets/[A-Za-z0-9_.-]*\.wasm' "$TMP_DIR/spa-wasm.js" | head -1 | sed 's|^/\./|/|')"
[[ -n "$spa_wasm" ]] || fail "JS 中未找到 wasm 路径"
curl -sf "http://127.0.0.1:$SERVER_PORT$spa_wasm" -o "$TMP_DIR/spa-wasm.bin"
wasm_bytes="$(wc -c < "$TMP_DIR/spa-wasm.bin")"
[[ "$wasm_bytes" -gt 1000 ]] || fail "$spa_wasm 内容过小（${wasm_bytes}B）"
printf 'GET %s → 200 OK (%s bytes)\n' "$spa_wasm" "$wasm_bytes"

# ---- 5. 管理接口（login 用同一 Bearer 校验）----
step "5/9 管理接口 /admin/config"
unauth_code="$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$SERVER_PORT/admin/config")"
[[ "$unauth_code" == "401" ]] || fail "无 token 访问 /admin/config 期望 401，实际 $unauth_code"

# 首次运行：引导创建管理员 → 登录拿会话
setup_out="$(curl -sf -X POST "http://127.0.0.1:$SERVER_PORT/admin/setup" \
  -H "Content-Type: application/json" \
  -d '{"username":"smoke","password":"smoke-pass-12345","password_confirm":"smoke-pass-12345"}')"
python3 -c 'import json,sys; d=json.load(sys.stdin); assert d["username"]=="smoke", d; print("setup OK")' <<<"$setup_out"

login_out="$(curl -sf -X POST "http://127.0.0.1:$SERVER_PORT/admin/login" \
  -H "Content-Type: application/json" \
  -d '{"username":"smoke","password":"smoke-pass-12345"}')"
ADMIN_TOKEN="$(python3 -c 'import json,sys; print(json.load(sys.stdin)["token"])' <<<"$login_out")"
[[ -n "$ADMIN_TOKEN" ]] || fail "login 未返回会话 token"

cfg="$(curl -sf "http://127.0.0.1:$SERVER_PORT/admin/config" -H "Authorization: Bearer $ADMIN_TOKEN")"
python3 -c 'import json,sys; d=json.load(sys.stdin); assert d["username"]=="smoke", d; print("config OK")' <<<"$cfg"
printf 'GET /admin/config（Bearer）→ 200 OK, 用户名一致\n'

# ---- 6. 加源 + 组合订阅 → 组合订阅输出 ----
step "6/9 加源与组合订阅 → /subscribe/{name}"
created="$(curl -sf -X POST "http://127.0.0.1:$SERVER_PORT/admin/sources" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  -d "{\"url\":\"http://127.0.0.1:$FIXTURE_PORT/sub.txt\",\"name\":\"fixture\"}")"
SRC_ID="$(python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])' <<<"$created")"

# 单条节点源（无网络依赖，指向必然失败的地址也不会被拉取）
created_single="$(curl -sf -X POST "http://127.0.0.1:$SERVER_PORT/admin/sources" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"url":"ss://YWVzLTI1Ni1nY206cGFzcw@h:8388#single-node","name":"single","kind":"single"}')"

# 组合勾选 fixture 源（single 源不入组合：订阅输出按组合成员过滤）
combined="$(curl -sf -X POST "http://127.0.0.1:$SERVER_PORT/admin/combineds" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  -d "{\"name\":\"merged\",\"source_ids\":[$SRC_ID]}")"
python3 -c 'import json,sys; d=json.load(sys.stdin); assert d["name"]=="merged"; assert d["source_ids"]==[int(sys.argv[1])], d; print("combined id=%d"%d["id"])' <<<"$combined" "$SRC_ID"

clash_out="$(curl -sf "http://127.0.0.1:$SERVER_PORT/subscribe/merged?format=clash")"
grep -q "fixture-node" <<<"$clash_out" || fail "/subscribe/merged 未输出 fixture-node"
grep -q "proxies:" <<<"$clash_out" || fail "/subscribe/merged 未输出 proxies 段"
printf 'GET /subscribe/merged?format=clash → 200 OK, 含 fixture-node\n'

# ---- 7. 组合订阅名不匹配 404 ----
step "7/9 错误组合名 404"
wrong_sub="$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$SERVER_PORT/subscribe/not-a-sub?format=clash")"
[[ "$wrong_sub" == "404" ]] || fail "错误组合名期望 404，实际 $wrong_sub"
printf 'GET /subscribe/not-a-sub → 404\n'

# ---- 8. 未知 API 命名空间 404 而非 SPA 回退 ----
step "8/9 未知 /admin/* 返回 JSON 404"
admin404="$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$SERVER_PORT/admin/nope")"
[[ "$admin404" == "404" ]] || fail "未知 /admin/* 期望 404，实际 $admin404"
printf 'GET /admin/nope → 404（不回退 SPA）\n'

# ---- 9. SPA 回退 ----
step "9/9 SPA 回退"
spa_fb="$(curl -sf "http://127.0.0.1:$SERVER_PORT/some/spa/route")"
grep -q "sub-merge" <<<"$spa_fb" || fail "SPA 回退未返回 index.html"
printf 'GET /some/spa/route → 回退 index.html\n'

printf '\n✅ 冒烟测试全部通过\n'
