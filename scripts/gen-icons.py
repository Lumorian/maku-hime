#!/usr/bin/env python3
"""生成 maku-hime 的应用图标（粉橙渐变 + 简化鸟居），无需 Pillow。

用法: python3 scripts/gen-icons.py
输出: src-tauri/icons/{32x32.png,128x128.png,icon.png}
"""
from __future__ import annotations

import math
import struct
import zlib
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "src-tauri" / "icons"
BRAND_START = (255, 102, 153)
BRAND_END = (255, 158, 109)


def mix(a, b, t: float):
    return tuple(round(x + (y - x) * t) for x, y in zip(a, b))


def rounded_alpha(x: float, y: float, size: float, radius: float) -> float:
    """圆角矩形的抗锯齿覆盖率（0..1）。"""
    cx = min(max(x, radius), size - radius)
    cy = min(max(y, radius), size - radius)
    dx = x - cx
    dy = y - cy
    distance = math.hypot(dx, dy)
    return max(0.0, min(1.0, radius - distance + 0.5))


def segment_distance(px, py, ax, ay, bx, by) -> float:
    vx, vy = bx - ax, by - ay
    wx, wy = px - ax, py - ay
    length_sq = vx * vx + vy * vy
    t = 0.0 if length_sq == 0 else max(0.0, min(1.0, (wx * vx + wy * vy) / length_sq))
    return math.hypot(px - (ax + t * vx), py - (ay + t * vy))


def render(size: int) -> bytes:
    scale = size / 128.0
    strokes = [
        # 上梁、下梁、两根立柱、中间短竖（简化鸟居）
        ((26, 42), (102, 42), 7.0),
        ((34, 58), (94, 58), 4.5),
        ((44, 46), (44, 100), 6.0),
        ((84, 46), (84, 100), 6.0),
        ((64, 44), (64, 74), 4.0),
    ]
    rows = []
    for py in range(size):
        row = bytearray()
        for px in range(size):
            x = (px + 0.5) / scale
            y = (py + 0.5) / scale
            t = (x / 128.0 + y / 128.0) / 2.0
            base = mix(BRAND_START, BRAND_END, min(1.0, t))
            # 右上角加一点高光
            glow = max(0.0, 1.0 - math.hypot(x - 96, y - 30) / 80.0) * 0.16
            r = min(255, round(base[0] + (255 - base[0]) * glow))
            g = min(255, round(base[1] + (255 - base[1]) * glow))
            b = min(255, round(base[2] + (255 - base[2]) * glow))

            alpha = rounded_alpha(x, y, 128.0, 26.0)
            # 白色鸟居
            cover = 0.0
            for (ax, ay), (bx, by), width in strokes:
                distance = segment_distance(x, y, ax, ay, bx, by)
                cover = max(cover, max(0.0, min(1.0, width / 2 - distance + 0.5)))
            if cover > 0:
                r = round(r + (255 - r) * cover)
                g = round(g + (255 - g) * cover)
                b = round(b + (255 - b) * cover)
            row += bytes((r, g, b, round(alpha * 255)))
        rows.append(bytes(row))
    return b"".join(rows)


def write_png(path: Path, size: int) -> None:
    raw = render(size)
    stride = size * 4
    filtered = b"".join(b"\x00" + raw[y * stride : (y + 1) * stride] for y in range(size))

    def chunk(tag: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(filtered, 9))
        + chunk(b"IEND", b"")
    )
    path.write_bytes(png)
    print(f"写出 {path} ({size}x{size}, {len(png)} 字节)")


if __name__ == "__main__":
    OUT.mkdir(parents=True, exist_ok=True)
    for name, size in (("32x32.png", 32), ("128x128.png", 128), ("icon.png", 256)):
        write_png(OUT / name, size)
