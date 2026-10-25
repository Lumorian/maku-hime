#!/usr/bin/env bash
# 开发用一键启动：本地 Rust 服务 + Vite 开发服务器（前端热更新）。
#   bash scripts/dev.sh [房间号]
# 前端: http://localhost:5173     接口: http://127.0.0.1:3939
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

ROOM="${1:-}"
PORT="${MAKUHIME_PORT:-3939}"

if ! command -v cargo >/dev/null; then
  echo "需要 Rust 工具链（cargo）" >&2
  exit 1
fi
if [ ! -d node_modules ]; then
  echo "首次运行，先安装前端依赖…"
  npm install --no-audit --no-fund
fi

cleanup() {
  [ -n "${SERVER_PID:-}" ] && kill "$SERVER_PID" 2>/dev/null || true
  [ -n "${VITE_PID:-}" ] && kill "$VITE_PID" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

ARGS=(--port "$PORT")
if [ -n "$ROOM" ]; then
  ARGS+=(--room "$ROOM")
fi

echo "启动本地服务 :$PORT …"
cargo run -q -p makuhime-server -- "${ARGS[@]}" &
SERVER_PID=$!

echo "启动 Vite …"
npx vite --port 5173 &
VITE_PID=$!

wait -n "$SERVER_PID" "$VITE_PID"
