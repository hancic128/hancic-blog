#!/usr/bin/env python3
"""把一张已烘焙的插图（PNG）插进 hancic 文章正文。

链路：
    assets/diagrams/*.svg  --(scripts/bake_diagrams.py)-->  assets/diagrams/png/*.png
    assets/diagrams/png/*.png  --(本脚本：上传附件 + 插入 <img> 图块)-->  文章正文

插图是往正文里**插入**块，不重打正文：上传 PNG 到附件库 → 按锚点插入 <figure>
→ 备份 → 校验「去掉图块后与原文逐字一致」→ PATCH 回写 → 回读再校验。
规范见 Trilium「中文博客插图规范」。

需要环境变量 HANCIC_BASE_URL、HANCIC_API_TOKEN（与 hancic-mcp 相同）。

用法：
    python3 scripts/insert_diagram.py -p 162 \\
        --png assets/diagrams/png/observability-three-scales.png \\
        --caption "三个工具，三个尺度" --after "三个都要。"
    # 默认只预览（不上传、不回写），确认无误再加 --write

退出码：0 成功（含「已存在，跳过」）；1 参数、校验或请求出错。
"""

from __future__ import annotations

import argparse
import datetime as dt
import html
import json
import mimetypes
import os
import re
import sys
import urllib.error
import urllib.request
import uuid
from pathlib import Path

# 内联样式：主题 CSS 里没有 .diagram 类，样式跟着图块走
FIGURE_STYLE = "margin:28px 0 32px"
IMG_STYLE = "display:block;width:100%;height:auto"
CAPTION_STYLE = "margin-top:12px;font-size:13px;line-height:1.5;text-align:center;color:var(--muted)"

# 图块 + 尾随空行，用于「去掉图块后与原文比对」；\r? 兼容 CRLF 正文
FIGURE_RE = re.compile(r'<figure class="diagram".*?</figure>\r?\n\r?\n', re.S)
# 单个图块（不含尾随空行），用于按图注定位：必须逐块比对，
# 否则 "<figure…>.*?<figcaption>图注</figcaption>" 会跨块匹配、连带吞掉前面的图
FIGURE_BLOCK_RE = re.compile(r'<figure class="diagram".*?</figure>', re.S)


class ApiError(RuntimeError):
    pass


def api(method: str, path: str, body: dict | None = None, raw: bytes | None = None,
        content_type: str = "application/json") -> dict:
    base = os.environ.get("HANCIC_BASE_URL", "").rstrip("/")
    token = os.environ.get("HANCIC_API_TOKEN", "")
    if not base or not token:
        raise ApiError("需要环境变量 HANCIC_BASE_URL 与 HANCIC_API_TOKEN（与 hancic-mcp 相同）")
    if raw is None and body is not None:
        raw = json.dumps(body).encode()
    headers = {"Authorization": f"Bearer {token}"}
    if raw is not None:
        headers["Content-Type"] = content_type
    request = urllib.request.Request(f"{base}/api{path}", data=raw, headers=headers, method=method)
    try:
        with urllib.request.urlopen(request, timeout=120) as resp:
            text = resp.read().decode()
            return json.loads(text) if text.strip() else {}
    except urllib.error.HTTPError as e:
        raise ApiError(f"{method} {path} -> HTTP {e.code}: {e.read().decode()[:300]}") from e
    except urllib.error.URLError as e:
        raise ApiError(f"{method} {path} -> {e}") from e


def unwrap(payload: dict) -> dict:
    return payload.get("data", payload)


def upload(png: Path) -> str:
    """上传 PNG 到附件库，返回正文里用的完整 URL。

    完整 URL（https://blog.hancic.site/uploads/…）而不是站内相对路径：
    文章要复制到公众号 / CSDN / 掘金等第三方平台，相对路径会被原样搬走
    （渲染成源码 / 破图），完整 URL 复制过去直接可访问。站内渲染两者都正常。
    """
    if not png.exists():
        raise ApiError(f"找不到 {png}；先跑 python3 scripts/bake_diagrams.py 烘焙")
    boundary = "----hancic" + uuid.uuid4().hex
    mime = mimetypes.guess_type(png.name)[0] or "application/octet-stream"
    head = (
        f"--{boundary}\r\n"
        f'Content-Disposition: form-data; name="files"; filename="{png.name}"\r\n'
        f"Content-Type: {mime}\r\n\r\n"
    ).encode()
    raw = head + png.read_bytes() + f"\r\n--{boundary}--\r\n".encode()
    payload = unwrap(api("POST", "/uploads", raw=raw,
                         content_type=f"multipart/form-data; boundary={boundary}"))
    items = payload if isinstance(payload, list) else [payload]
    if not items or not isinstance(items[0], dict) or "path" not in items[0]:
        raise ApiError(f"上传返回异常：{str(payload)[:200]}")
    base = os.environ.get("HANCIC_BASE_URL", "").rstrip("/")
    return f"{base}/uploads/{items[0]['path']}"


def newline_of(md: str) -> str:
    """正文的换行风格：CRLF 正文按原样插入，免得插图顺手把行尾改成另一种。"""
    return "\r\n" if "\r\n" in md else "\n"


def build_figure(url: str, caption: str, alt: str, nl: str = "\n") -> str:
    return nl.join([
        f'<figure class="diagram" style="{FIGURE_STYLE}">',
        f'<img src="{url}" alt="{html.escape(alt)}" style="{IMG_STYLE}">',
        f'<figcaption style="{CAPTION_STYLE}">{html.escape(caption)}</figcaption>',
        "</figure>",
    ])


def by_caption(md: str, caption: str) -> list[re.Match]:
    marker = f">{html.escape(caption)}</figcaption>"
    return [m for m in FIGURE_BLOCK_RE.finditer(md) if marker in m.group(0)]


def insert(md: str, anchor: str, block: str) -> str:
    hits = md.count(anchor)
    if hits != 1:
        raise ApiError(f"锚点命中 {hits} 次，必须唯一：{anchor[:40]}")
    end = md.find(anchor) + len(anchor)
    nl = newline_of(md)
    if not md.startswith(nl + nl, end):
        raise ApiError("锚点必须落在段落末尾、后面紧跟空行，换一个锚点")
    return md[:end] + nl + nl + block + md[end:]


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="把一张已烘焙的 PNG 插图插进 hancic 文章正文")
    parser.add_argument("-p", "--post", type=int, required=True, help="文章 id")
    parser.add_argument("--png", type=Path, required=True, help="烘焙好的 PNG（assets/diagrams/png/…）")
    parser.add_argument("--caption", required=True, help="图注（3–5 个词），同时作为 --replace 的定位键")
    parser.add_argument("--alt", help="图片 alt 文本，默认同图注")
    parser.add_argument("--after", help="锚点：图块插在这个段落之后，必须唯一（--replace 时不需要）")
    parser.add_argument("--replace", action="store_true", help="按图注替换已有图块（改图用）")
    parser.add_argument("--write", action="store_true", help="真上传并回写（默认只预览）")
    parser.add_argument("--force", action="store_true", help="同图注图块已存在时也再插一个")
    parser.add_argument("--backup-dir", type=Path, default=Path("/root/backups"), help="原文备份目录")
    args = parser.parse_args(argv[1:])
    if not args.replace and not args.after:
        parser.error("需要 --after 指定锚点，或用 --replace 替换已有图块")

    alt = args.alt or args.caption
    post = unwrap(api("GET", f"/posts/{args.post}"))
    old_md = post["content_md"]
    existing = by_caption(old_md, args.caption)

    if args.replace:
        if len(existing) != 1:
            raise ApiError(f"--replace 按图注「{args.caption}」命中 {len(existing)} 个图块，必须唯一")
        action = "替换"
    else:
        if existing and not args.force:
            print(f"文章 {args.post} 里已有图注「{args.caption}」的图块，跳过（改图用 --replace，确实要再插一张用 --force）")
            return 0
        action = "插入"

    print(f"文章 {args.post}「{post['title']}」，状态 {post['status']}")
    print(f"{action}位置：" + (f"图注「{args.caption}」所在图块" if args.replace
                              else f"锚点段落「{args.after[:24]}…」之后"))
    print(f"图注：{args.caption}｜alt：{alt}")
    print(f"图片：{args.png}（{args.png.stat().st_size if args.png.exists() else '不存在'} 字节）")
    if args.replace:
        old_src = re.search(r'<img src="([^"]+)"', existing[0].group(0))
        print(f"被替换的图块当前引用：{old_src.group(1) if old_src else '（非 <img> 图块）'}")

    if not args.write:
        print("\n预览模式：未上传附件、未回写。确认无误加 --write")
        return 0

    url = upload(args.png)
    block = build_figure(url, args.caption, alt, newline_of(old_md))
    if args.replace:
        new_md = old_md[: existing[0].start()] + block + old_md[existing[0].end() :]
    else:
        new_md = insert(old_md, args.after, block)
    if FIGURE_RE.sub("", new_md) != FIGURE_RE.sub("", old_md):
        raise ApiError(f"{action}会改动图块以外的正文，已中止（附件 {url} 已上传，可自行删除）")

    args.backup_dir.mkdir(parents=True, exist_ok=True)
    stamp = dt.datetime.now().strftime("%Y%m%d-%H%M%S")
    backup = args.backup_dir / f"hancic-{args.post}-content-md-{stamp}-pre-diagram.md"
    backup.write_text(old_md, encoding="utf-8")

    api("PATCH", f"/posts/{args.post}", {"content_md": new_md})

    check = unwrap(api("GET", f"/posts/{args.post}"))
    got = check["content_md"]
    if FIGURE_RE.sub("", got) != FIGURE_RE.sub("", old_md):
        raise ApiError(f"回读校验失败：去掉图块后与原文不一致，原文备份在 {backup}")
    before, after = len(FIGURE_RE.findall(old_md)), len(FIGURE_RE.findall(got))
    expected = before + (0 if args.replace else 1)
    if after != expected:
        raise ApiError(f"回读校验失败：图块数量 {before} → {after}，期望 {expected}，原文备份在 {backup}")
    print(f"已回写，回读校验通过（图块 {before} → {after}，图块以外逐字未动）。")
    print(f"附件 URL：{url}")
    print(f"备份：{backup}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main(sys.argv))
    except ApiError as e:
        print(f"错误：{e}", file=sys.stderr)
        raise SystemExit(1)
