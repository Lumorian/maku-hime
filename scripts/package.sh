#!/usr/bin/env bash
# 打一个「解压即用」的 tar.gz：桌面端 + 可单独跑的本地服务 + 前端静态资源。
# 用法: bash scripts/package.sh [输出目录]
# 产物: dist-packages/maku-hime-<version>-linux-<arch>.tar.gz
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

OUT_DIR="${1:-$ROOT/dist-packages}"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' src-tauri/Cargo.toml | head -1)"
ARCH="$(uname -m)"
NAME="maku-hime-${VERSION}-linux-${ARCH}"
STAGE="${OUT_DIR}/${NAME}"

if [ ! -d node_modules ]; then
  echo "缺少 node_modules，先执行 npm install" >&2
  exit 1
fi

echo "==> 构建前端"
npm run build

echo "==> 构建 release 二进制（首次会比较久，开了 LTO）"
cargo build --release -p maku-hime -p makuhime-server

echo "==> 组装 ${NAME}"
rm -rf "${STAGE}"
mkdir -p "${STAGE}"
cp target/release/maku-hime "${STAGE}/"
cp target/release/maku-hime-server "${STAGE}/"
cp -r dist "${STAGE}/dist"
cp README.md LICENSE "${STAGE}/"
chmod 755 "${STAGE}/maku-hime" "${STAGE}/maku-hime-server"
chmod 644 "${STAGE}/README.md" "${STAGE}/LICENSE"
chmod -R a+rX "${STAGE}/dist"

cat > "${STAGE}/启动说明.txt" <<'TXT'
maku-hime（幕姬）· 解压即用

桌面应用:
    ./maku-hime                 # 打开控制面板
    ./maku-hime --overlay       # 同时打开悬浮窗

只跑本地服务（浏览器面板 / OBS 浏览器源）:
    ./maku-hime-server --room <房间号> --open
    OBS 浏览器源地址: http://127.0.0.1:3939/obs?room=<房间号>

配置与历史:
    ~/.config/maku-hime/config.toml
    ~/.local/share/maku-hime/history/<房间号>.jsonl

Wayland 穿透无效时可以用 XWayland 启动:
    GDK_BACKEND=x11 ./maku-hime --overlay
TXT

tar -C "${OUT_DIR}" -czf "${STAGE}.tar.gz" "${NAME}"
echo "==> 完成: ${STAGE}.tar.gz"
du -h "${STAGE}.tar.gz"
