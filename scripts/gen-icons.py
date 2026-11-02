#!/usr/bin/env python3
"""从设计稿生成全站图标。

母版：`docs/logo/maku-hime-logo.png`（设计师交付的 1254×1254 源图，白底 + 圆角渐变徽章）。
本脚本只做几何处理，不重绘图形：

1. 按饱和度找出徽章的实际边界，裁掉四周的白边；
2. 用与母版一致的圆角（边长 19.2%）生成 alpha，得到"透明角"的方形图标；
3. 派生 Web 用的 favicon / apple-touch-icon / 面板里的 28px 小标；
4. 调用 `npx tauri icon` 生成 `src-tauri/icons/` 全套（含 .ico / .icns）。

用法：`python3 scripts/gen-icons.py`（需要 Pillow；没有就先 `pip install pillow`）
输出（除 src-tauri/icons 外都会被 git 跟踪）：
  docs/logo/icon-1024.png   图标母版（透明角），tauri icon 的输入
  src/assets/logo.png       面板侧栏用的 256px 品牌标
  public/favicon.png        浏览器标签页
  public/favicon.ico        老浏览器 / 系统识别
  public/apple-touch-icon.png
  src-tauri/icons/*         桌面壳、托盘、AppImage、安装包
"""

from __future__ import annotations

import shutil
import subprocess
import sys
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "docs" / "logo" / "maku-hime-logo.png"
ICON_MASTER = ROOT / "docs" / "logo" / "icon-1024.png"

# 圆角半径 / 边长。母版实测 222.5 / 1160，拟合残差 0.27px。
RADIUS_RATIO = 0.192
# 蒙版往里收一点点，盖掉母版边缘"渐变混白底"的一像素过渡。
MASK_INSET = 0.6
SUPERSAMPLE = 4


def badge_box(image: Image.Image) -> tuple[int, int, int, int]:
    """按饱和度找出渐变徽章的外接矩形（母版四周有白边，不能直接整张缩）。"""
    rgb = image.convert("RGB")
    width, height = rgb.size
    sat = [max(p) - min(p) for p in rgb.getdata()]
    left, top, right, bottom = width, height, -1, -1
    for index, value in enumerate(sat):
        if value <= 25:
            continue
        x, y = index % width, index // width
        left, top = min(left, x), min(top, y)
        right, bottom = max(right, x), max(bottom, y)
    if right < 0:
        raise SystemExit("母版里找不到渐变徽章，请确认 docs/logo/maku-hime-logo.png 没被换掉")
    return left, top, right + 1, bottom + 1


def rounded_mask(size: int, radius_ratio: float = RADIUS_RATIO) -> Image.Image:
    """抗锯齿的圆角方块蒙版（白 = 保留）。"""
    big = size * SUPERSAMPLE
    radius = (radius_ratio * size - MASK_INSET) * SUPERSAMPLE
    mask = Image.new("L", (big, big), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, big - 1, big - 1), radius=radius, fill=255)
    return mask.resize((size, size), Image.LANCZOS)


def squircle(badge: Image.Image, size: int, transparent: bool = True) -> Image.Image:
    """把裁好的徽章缩到目标尺寸；transparent=True 时圆角外是透明的。"""
    scaled = badge.resize((size, size), Image.LANCZOS).convert("RGBA")
    if transparent:
        scaled.putalpha(rounded_mask(size))
    return scaled


def main() -> int:
    if not SRC.is_file():
        print(f"找不到母版 {SRC}", file=sys.stderr)
        return 1
    source = Image.open(SRC)
    badge = source.crop(badge_box(source))
    print(f"母版 {source.size[0]}×{source.size[1]}，徽章 {badge.size[0]}×{badge.size[1]}")

    ICON_MASTER.parent.mkdir(parents=True, exist_ok=True)
    squircle(badge, 1024).save(ICON_MASTER)
    print(f"写出 {ICON_MASTER.relative_to(ROOT)}")

    assets = ROOT / "src" / "assets"
    assets.mkdir(parents=True, exist_ok=True)
    squircle(badge, 192).save(assets / "logo.png", optimize=True)
    print(f"写出 src/assets/logo.png（{192}px，侧栏 28px 的 6.8×）")

    public = ROOT / "public"
    public.mkdir(exist_ok=True)
    squircle(badge, 64).save(public / "favicon.png", optimize=True)
    squircle(badge, 180, transparent=False).save(public / "apple-touch-icon.png")
    squircle(badge, 64).save(public / "favicon.ico", sizes=[(16, 16), (32, 32), (48, 48)])
    print("写出 public/favicon.png、public/favicon.ico、public/apple-touch-icon.png")

    tauri = ROOT / "node_modules" / ".bin" / "tauri"
    if tauri.is_file() and shutil.which("node"):
        print("调用 npx tauri icon 生成 src-tauri/icons …")
        subprocess.run(
            ["npx", "tauri", "icon", str(ICON_MASTER)],
            cwd=ROOT,
            check=True,
        )
        # 本项目只出 deb / AppImage，桌面图标够用；tauri icon 顺手生成的
        # 移动端资源（android/ ios/）用不上，别让它进仓库。
        for mobile in ("android", "ios"):
            target = ROOT / "src-tauri" / "icons" / mobile
            if target.is_dir():
                shutil.rmtree(target)
        print("已清理 src-tauri/icons/{android,ios}（桌面版用不到）")
    else:
        print("没装 @tauri-apps/cli，跳过桌面图标；需要时手动跑：")
        print("  npx tauri icon docs/logo/icon-1024.png")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
