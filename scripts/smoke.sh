#!/usr/bin/env bash
# sub-merge 端到端冒烟测试
#
# 验证链路：
#   0. cargo build -p server（生成可执行二进制）
#   1. dx build --web 构建前端 → dist/
#   2. server 以 WEB_DIST 指向 dist 启动（临时 DB、随机端口）
#   3. curl 根路径（health）→ "sub-merge is running"
#   4. curl 静态资源 index.html / wasm js / wasm binary → 200
#   5. /api/admin/config（Bearer）→ 返回 subscribe_token/admin_token
#   6. 加一个本地 fixture 订阅源 → /api/subscribe?token=..&format=clash 输出节点
#   7. 未授权 subscribe → 401
#   8. SPA 回退
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
step "1/9 dx build --web --release"
(
  cd "$ROOT/crates/server/web"
  dx build --web --release
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
step "5/9 管理接口 /api/admin/config"
unauth_code="$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$SERVER_PORT/api/admin/config")"
[[ "$unauth_code" == "401" ]] || fail "无 token 访问 /api/admin/config 期望 401，实际 $unauth_code"

# 从日志拿不到 token（debug 级别），直接查 DB 的 settings 表拿 admin/subscribe token
ADMIN_TOKEN="$(python3 - "$TMP_DIR/submerge-smoke.db" <<'PY'
import sqlite3, sys
db = sqlite3.connect(sys.argv[1])
print(db.execute("SELECT value FROM settings WHERE key='admin_token'").fetchone()[0])
PY
)"
SUB_TOKEN="$(python3 - "$TMP_DIR/submerge-smoke.db" <<'PY'
import sqlite3, sys
db = sqlite3.connect(sys.argv[1])
print(db.execute("SELECT value FROM settings WHERE key='subscribe_token'").fetchone()[0])
PY
)"
[[ -n "$ADMIN_TOKEN" && -n "$SUB_TOKEN" ]] || fail "DB 中未生成 token"
cfg="$(curl -sf "http://127.0.0.1:$SERVER_PORT/api/admin/config" -H "Authorization: Bearer $ADMIN_TOKEN")"
python3 -c 'import json,sys; d=json.load(sys.stdin); assert d["subscribe_token"]==sys.argv[1], "config token 不匹配"; assert d["admin_token"]==sys.argv[2]; print("config 返回 token 一致")' <<<"$cfg" "$SUB_TOKEN" "$ADMIN_TOKEN"
printf 'GET /api/admin/config（Bearer）→ 200 OK, token 一致\n'

# ---- 6. 加订阅源 → 订阅输出 ----
step "6/9 加订阅源 → /api/subscribe"
created="$(curl -sf -X POST "http://127.0.0.1:$SERVER_PORT/api/admin/sources" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  -d "{\"url\":\"http://127.0.0.1:$FIXTURE_PORT/sub.txt\",\"name\":\"fixture\"}")"
python3 -c 'import json,sys; d=json.load(sys.stdin); assert d["enabled"]==True; assert d["name"]=="fixture"; print("source id=%d"%d["id"])' <<<"$created"

clash_out="$(curl -sf "http://127.0.0.1:$SERVER_PORT/api/subscribe?token=$SUB_TOKEN&format=clash")"
grep -q "fixture-node" <<<"$clash_out" || fail "/api/subscribe 未输出 fixture-node"
grep -q "proxies:" <<<"$clash_out" || fail "/api/subscribe 未输出 proxies 段"
printf 'GET /api/subscribe?format=clash → 200 OK, 含 fixture-node\n'

# ---- 7. 未授权 subscribe ----
step "7/9 未授权 subscribe 拒绝"
unauth_sub="$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$SERVER_PORT/api/subscribe?token=wrong&format=clash")"
[[ "$unauth_sub" == "401" ]] || fail "错误 token 期望 401，实际 $unauth_sub"
printf 'GET /api/subscribe（错误 token）→ 401\n'

# ---- 8. 未知 API 404 而非 SPA 回退 ----
step "8/9 未知 /api/* 返回 JSON 404"
api404="$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$SERVER_PORT/api/nope")"
[[ "$api404" == "404" ]] || fail "未知 /api/* 期望 404，实际 $api404"
printf 'GET /api/nope → 404（不回退 SPA）\n'

# ---- 9. SPA 回退 ----
step "9/9 SPA 回退"
spa_fb="$(curl -sf "http://127.0.0.1:$SERVER_PORT/some/spa/route")"
grep -q "sub-merge" <<<"$spa_fb" || fail "SPA 回退未返回 index.html"
printf 'GET /some/spa/route → 回退 index.html\n'

printf '\n✅ 冒烟测试全部通过\n'
