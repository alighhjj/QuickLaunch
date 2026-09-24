#!/usr/bin/env python3
"""生成 QuickLaunch 的全部图标资源。

Tauri 打包 macOS 需要 `.icns`，而 Pillow 在非 macOS 平台上无法可靠地写 ICNS，
因此这里直接按 Apple Icon Image 容器格式手工封装分块：
`icns` 魔数 + 文件总长 + 若干 (4 字节类型 + 4 字节含头长度 + PNG 数据) 分块。
现代 macOS 完全支持 PNG 负载的分块，所以无需做任何像素格式转换。

用法：
    python scripts/make_icons.py
输出目录默认为 src-tauri/icons。
"""

from __future__ import annotations

import io
import struct
import sys
from pathlib import Path

from PIL import Image, ImageDraw

# ── 设计常量 ────────────────────────────────────────────────
MASTER = 2048
GRADIENT_TOP = (92, 166, 255)
GRADIENT_BOTTOM = (34, 74, 210)
LENS_CENTER = (0.420, 0.402)
LENS_RADIUS = 0.238
RING_STROKE = 0.076
HANDLE_STROKE = 0.088
BOLT_SCALE = 0.150

# ICNS 分块类型 → 像素边长
ICNS_CHUNKS: list[tuple[str, int]] = [
    ("ic11", 32),    # 16pt @2x
    ("ic12", 64),    # 32pt @2x
    ("ic07", 128),   # 128pt
    ("ic13", 256),   # 128pt @2x
    ("ic08", 256),   # 256pt
    ("ic09", 512),   # 512pt
    ("ic14", 512),   # 256pt @2x
    ("ic10", 1024),  # 512pt @2x
]

ICO_SIZES = [(16, 16), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]


def gradient_sheet(size: int) -> Image.Image:
    """竖直渐变。逐行生成一条 1px 宽的色带再拉伸，避免逐像素写入 400 万次。"""
    strip = Image.new("RGB", (1, size))
    for y in range(size):
        t = (y / (size - 1)) ** 0.85
        strip.putpixel(
            (
                0,
                y,
            ),
            tuple(
                round(GRADIENT_TOP[i] + (GRADIENT_BOTTOM[i] - GRADIENT_TOP[i]) * t)
                for i in range(3)
            ),
        )
    return strip.resize((size, size), Image.NEAREST)


def bolt_polygon(center: tuple[float, float], scale: float) -> list[tuple[float, float]]:
    """以镜头中心为原点的闪电轮廓，归一化坐标 × scale。"""
    shape = [
        (0.06, -0.62),
        (-0.34, 0.04),
        (-0.04, 0.04),
        (-0.16, 0.62),
        (0.34, -0.06),
        (0.04, -0.06),
    ]
    cx, cy = center
    return [(cx + x * scale, cy + y * scale) for x, y in shape]


def render_app_icon(size: int = MASTER) -> Image.Image:
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))

    # 圆角矩形底：半径取 22.5%，接近 macOS Big Sur 之后的图标轮廓
    mask = Image.new("L", (size, size), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        [0, 0, size - 1, size - 1],
        radius=round(size * 0.225),
        fill=255,
    )
    canvas.paste(gradient_sheet(size), (0, 0), mask)

    draw = ImageDraw.Draw(canvas)
    cx, cy = LENS_CENTER[0] * size, LENS_CENTER[1] * size
    radius = LENS_RADIUS * size
    ring = RING_STROKE * size

    # 手柄先画，让镜圈盖住接缝
    handle_start = (cx + radius * 0.60, cy + radius * 0.60)
    handle_end = (cx + radius * 1.42, cy + radius * 1.42)
    draw.line(
        [handle_start, handle_end],
        fill=(255, 255, 255, 255),
        width=round(HANDLE_STROKE * size),
    )
    draw.ellipse(
        [
            handle_end[0] - HANDLE_STROKE * size / 2,
            handle_end[1] - HANDLE_STROKE * size / 2,
            handle_end[0] + HANDLE_STROKE * size / 2,
            handle_end[1] + HANDLE_STROKE * size / 2,
        ],
        fill=(255, 255, 255, 255),
    )

    # 镜圈
    draw.ellipse(
        [cx - radius, cy - radius, cx + radius, cy + radius],
        outline=(255, 255, 255, 255),
        width=round(ring),
    )

    # 镜内闪电
    draw.polygon(
        bolt_polygon((cx, cy), BOLT_SCALE * size),
        fill=(255, 255, 255, 255),
    )
    return canvas


def render_tray_icon(size: int = 32) -> Image.Image:
    """菜单栏模板图标：纯黑 + alpha，由 macOS 按明暗自动反色。"""
    scale = 8
    canvas = Image.new("RGBA", (size * scale, size * scale), (0, 0, 0, 0))
    draw = ImageDraw.Draw(canvas)
    s = size * scale

    cx, cy = 0.415 * s, 0.395 * s
    radius = 0.255 * s
    ring = max(2, round(0.085 * s))
    draw.line(
        [(cx + radius * 0.62, cy + radius * 0.62), (cx + radius * 1.38, cy + radius * 1.38)],
        fill=(0, 0, 0, 255),
        width=round(ring * 1.05),
    )
    draw.ellipse(
        [cx - radius, cy - radius, cx + radius, cy + radius],
        outline=(0, 0, 0, 255),
        width=ring,
    )
    return canvas.resize((size, size), Image.LANCZOS)


def write_icns(path: Path, master: Image.Image) -> None:
    chunks = bytearray()
    for kind, side in ICNS_CHUNKS:
        buffer = io.BytesIO()
        master.resize((side, side), Image.LANCZOS).save(buffer, format="PNG", optimize=True)
        payload = buffer.getvalue()
        chunks += kind.encode("ascii") + struct.pack(">I", len(payload) + 8) + payload

    total = 8 + len(chunks)
    path.write_bytes(b"icns" + struct.pack(">I", total) + bytes(chunks))


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    out = root / "src-tauri" / "icons"
    out.mkdir(parents=True, exist_ok=True)

    master = render_app_icon()

    for side, name in [
        (32, "32x32.png"),
        (128, "128x128.png"),
        (256, "128x128@2x.png"),
        (1024, "icon.png"),
    ]:
        master.resize((side, side), Image.LANCZOS).save(out / name, format="PNG", optimize=True)

    write_icns(out / "icon.icns", master)
    master.resize((256, 256), Image.LANCZOS).save(
        out / "icon.ico", format="ICO", sizes=ICO_SIZES
    )
    render_tray_icon(32).save(out / "tray.png", format="PNG", optimize=True)

    for produced in sorted(out.iterdir()):
        print(f"  {produced.name:>18}  {produced.stat().st_size:>8} bytes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
