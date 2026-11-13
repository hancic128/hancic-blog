#!/usr/bin/env python3
"""把 assets/diagrams/*.svg 烘焙成 assets/diagrams/png/*.png。

正文里的插图是内联 SVG，靠 currentColor 跟随亮/暗主题；PNG 是另一条链路
（站外引用、不能渲染 SVG 的客户端），所以这里出一份颜色冻结、垫了白底卡片
的位图，画布和 SVG 一致，只是放大到 2x。

本机没有系统字体，也没有 resvg/rsvg-convert/inkscape，所以光栅化交给
scripts/raster 的 npm 依赖（@resvg/resvg-js），中文字体（Noto Sans SC）在
首次运行时下载到 scripts/raster/fonts/ 并缓存（该目录不入库）。

用法：
    python3 scripts/bake_diagrams.py                    # 烘焙 assets/diagrams/*.svg
    python3 scripts/bake_diagrams.py observability-three-scales.svg   # 只烘这一张
退出码：0 全部成功，1 有失败。
"""

from __future__ import annotations

import glob
import os
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.error
import urllib.request

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DIAGRAM_SRC = os.path.join(REPO_ROOT, "assets", "diagrams")
DIAGRAM_OUT = os.path.join(DIAGRAM_SRC, "png")
INK = "#1e1e1e"
PAPER = "#ffffff"

# Rasteriser. No fonts and no cairo/rsvg/inkscape on this host, so resvg ships as a pinned
# npm dependency (scripts/raster) and the CJK faces are fetched on first bake.
RASTER_DIR = os.path.join(REPO_ROOT, "scripts", "raster")
RENDER_JS = os.path.join(RASTER_DIR, "render.mjs")
FONT_DIR = os.path.join(RASTER_DIR, "fonts")
FONT_VERSION = "0.4.3"
FONT_SPECS = (
    ("400Regular", "NotoSansSC_400Regular.ttf"),
    ("600SemiBold", "NotoSansSC_600SemiBold.ttf"),
)
FONT_URL = "https://cdn.jsdelivr.net/npm/@expo-google-fonts/noto-sans-sc@{v}/{w}/{n}.ttf"
RASTER_ZOOM = 2  # the blog shows them at 720 CSS px, so 2x matches a retina screen


def _bake_svg(svg: str) -> str:
    """Freeze currentColor to a fixed ink and lay a white card behind the drawing."""
    box = re.search(r'viewBox="[-\d.]+ [-\d.]+ ([\d.]+) ([\d.]+)"', svg)
    if not box:
        raise ValueError("viewBox missing")
    width, height = box.groups()
    card = f'<rect x="0" y="0" width="{width}" height="{height}" fill="{PAPER}"/>'
    svg = re.sub(r"(<svg\b[^>]*>)", lambda m: f"{m.group(1)}\n  {card}", svg, count=1)
    return svg.replace("currentColor", INK)


def _ensure_fonts() -> None:
    """Fetch the Noto Sans SC faces on first use. resvg draws no text without them."""
    os.makedirs(FONT_DIR, exist_ok=True)
    for weight, name in FONT_SPECS:
        path = os.path.join(FONT_DIR, name)
        if os.path.exists(path) and open(path, "rb").read(4) == b"\x00\x01\x00\x00":
            continue
        url = FONT_URL.format(v=FONT_VERSION, w=weight, n=name[:-4])
        try:
            with urllib.request.urlopen(url, timeout=120) as resp, open(path, "wb") as f:
                shutil.copyfileobj(resp, f)
        except (urllib.error.URLError, OSError) as exc:
            raise RuntimeError(f"cannot fetch {name} from {url}: {exc}") from exc
        if open(path, "rb").read(4) != b"\x00\x01\x00\x00":
            os.remove(path)
            raise RuntimeError(f"{url} did not return a TrueType font")


def _rasterise(svg: str, out_path: str) -> tuple[int, int]:
    """Render baked SVG to PNG and return its pixel size."""
    if shutil.which("node") is None:
        raise RuntimeError("node not found; install it to bake diagrams")
    if not os.path.isdir(os.path.join(RASTER_DIR, "node_modules", "@resvg")):
        raise RuntimeError(f"run `npm install` in {RASTER_DIR} first")
    _ensure_fonts()
    with tempfile.TemporaryDirectory() as tmp:
        source = os.path.join(tmp, "in.svg")
        with open(source, "w", encoding="utf-8") as f:
            f.write(svg)
        proc = subprocess.run(
            ["node", RENDER_JS, source, out_path, str(RASTER_ZOOM)],
            cwd=RASTER_DIR,
            capture_output=True,
            text=True,
            timeout=180,
        )
    if proc.returncode != 0:
        raise RuntimeError(f"resvg failed: {proc.stderr.strip()[:300]}")
    return _png_size(out_path)


def _png_size(path: str) -> tuple[int, int]:
    """Width and height from the PNG IHDR chunk."""
    with open(path, "rb") as f:
        head = f.read(24)
    if head[:8] != b"\x89PNG\r\n\x1a\n":
        raise RuntimeError(f"{path} is not a PNG")
    return int.from_bytes(head[16:20], "big"), int.from_bytes(head[20:24], "big")


def _sources(argv: list[str]) -> list[str]:
    """All diagrams, or the named ones (matched inside assets/diagrams/)."""
    if not argv:
        return sorted(glob.glob(os.path.join(DIAGRAM_SRC, "*.svg")))
    paths = []
    for arg in argv:
        path = arg if os.path.sep in arg else os.path.join(DIAGRAM_SRC, arg)
        if not os.path.exists(path):
            raise RuntimeError(f"{path} not found")
        paths.append(path)
    return paths


def main(argv: list[str]) -> int:
    try:
        sources = _sources(argv)
    except RuntimeError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1
    if not sources:
        print(f"error: no SVG under {DIAGRAM_SRC}", file=sys.stderr)
        return 1

    os.makedirs(DIAGRAM_OUT, exist_ok=True)
    failed = 0
    for src in sources:
        out_path = os.path.join(DIAGRAM_OUT, os.path.basename(src)[:-4] + ".png")
        with open(src, encoding="utf-8") as f:
            svg = f.read()
        try:
            width, height = _rasterise(_bake_svg(svg), out_path)
        except (RuntimeError, ValueError) as exc:
            print(f"FAIL {src}: {exc}", file=sys.stderr)
            failed += 1
            continue
        size = os.path.getsize(out_path)
        print(f"{out_path}  {width}x{height}px  {size / 1024:.1f} KiB")

    if failed:
        print(f"{failed}/{len(sources)} diagram(s) failed", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
