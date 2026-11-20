#!/usr/bin/env python3
"""插图几何检查：assets/diagrams/*.svg。

hancic 没有构建期插图管线——图直接内联进文章的 content_md，样式问题只能靠
几何检查兜住（渲染目检另走 scripts/bake_diagrams.py 烘出的 PNG，那台机器上
没有系统字体，所以那条链路自带 Noto Sans SC）。发布前能自动化的就是这一层：

  1. SVG 是否 XML 合法（语法错浏览器直接空白）
  2. 文字是否越出画布
  3. 文字是否溢出所在的框
  4. 文字是否压住别的框或线段
  5. 文本两两是否重叠

文字宽度按字符宽度估算（CJK/全角 = 1em，其余 = 0.6em，空格 = 0.3em），
估算值比真实字宽略宽，偏向保守。

用法：
    python3 scripts/check_diagrams.py                      # 检查 assets/diagrams/*.svg
    python3 scripts/check_diagrams.py path/to/a.svg ...     # 检查指定文件
退出码：0 全部通过，1 有问题。
"""

from __future__ import annotations

import glob
import sys
import unicodedata
import xml.etree.ElementTree as ET
from pathlib import Path

NS = "{http://www.w3.org/2000/svg}"
DEFAULT_GLOB = "assets/diagrams/*.svg"


def text_width(s: str, font_size: float) -> float:
    """按字符类别估算文本宽度（偏宽，保守）。"""
    total = 0.0
    for ch in s:
        if ch == " ":
            total += 0.3 * font_size
        elif unicodedata.east_asian_width(ch) in ("W", "F"):
            total += 1.0 * font_size
        else:
            total += 0.6 * font_size
    return total


def box(elem, *names):
    return tuple(float(elem.get(n)) for n in names)


def check(path: Path) -> list[str]:
    problems: list[str] = []
    try:
        root = ET.parse(path).getroot()
    except ET.ParseError as e:
        return [f"{path}: XML 非法 -> {e}"]

    view_box = root.get("viewBox")
    if not view_box:
        return [f"{path}: 缺少 viewBox"]
    _, _, width, height = (float(v) for v in view_box.split())
    if width != 720:
        problems.append(f"{path}: viewBox 宽 {width:.0f}，规范要求 720（正文列宽）")

    rects = [box(r, "x", "y") + box(r, "width", "height") for r in root.iter(NS + "rect")]
    rects = [(x, y, x + w, y + h) for x, y, w, h in rects]
    lines = []
    for line in root.iter(NS + "line"):
        x1, y1, x2, y2 = box(line, "x1", "y1", "x2", "y2")
        lines.append((min(x1, x2) - 1, min(y1, y2) - 1, max(x1, x2) + 1, max(y1, y2) + 1))

    texts: list[tuple[str, tuple[float, float, float, float]]] = []
    for t in root.iter(NS + "text"):
        s = (t.text or "").strip()
        if not s:
            continue
        x, y, fs = box(t, "x", "y", "font-size")
        w = text_width(s, fs)
        x0 = x - w / 2 if t.get("text-anchor") == "middle" else x
        # 基线往上下各取近似行高
        texts.append((s, (x0, y - 0.8 * fs, x0 + w, y + 0.28 * fs)))

    def overlap(a, b) -> tuple[float, float]:
        return (min(a[2], b[2]) - max(a[0], b[0]), min(a[3], b[3]) - max(a[1], b[1]))

    for s, tb in texts:
        x0, y0, x1, y1 = tb
        if x0 < 0 or x1 > width or y0 < 0 or y1 > height:
            problems.append(f"{path}: 文字「{s}」越出画布（{x0:.0f},{y0:.0f})-({x1:.0f},{y1:.0f}) viewBox {width:.0f}x{height:.0f}")
        for i, r in enumerate(rects):
            ox, oy = overlap(tb, r)
            if ox > 1 and oy > 1 and not (x0 >= r[0] and x1 <= r[2] and y0 >= r[1] and y1 <= r[3]):
                problems.append(f"{path}: 文字「{s}」压住第 {i + 1} 个框或溢出其边界")
        for i, seg in enumerate(lines):
            ox, oy = overlap(tb, seg)
            if ox > 1 and oy > 1:
                problems.append(f"{path}: 文字「{s}」压住第 {i + 1} 条线")

    for i in range(len(texts)):
        for j in range(i + 1, len(texts)):
            ox, oy = overlap(texts[i][1], texts[j][1])
            if ox > 2 and oy > 2:
                problems.append(f"{path}: 文字「{texts[i][0]}」与「{texts[j][0]}」重叠 {ox:.0f}x{oy:.0f}px")

    if not problems:
        print(f"{path}: OK（文字 {len(texts)}，框 {len(rects)}，线 {len(lines)}，viewBox {width:.0f}x{height:.0f}）")
    return problems


def main(argv: list[str]) -> int:
    targets = argv[1:] or sorted(glob.glob(DEFAULT_GLOB))
    if not targets:
        print(f"没找到待检查的 SVG（默认 {DEFAULT_GLOB}）", file=sys.stderr)
        return 1
    all_problems: list[str] = []
    for target in targets:
        all_problems += check(Path(target))
    if all_problems:
        print("\n".join(all_problems), file=sys.stderr)
        print(f"\n{len(all_problems)} 个问题", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
