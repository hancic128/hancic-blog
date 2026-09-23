#!/usr/bin/env python3
"""把工作日 10:00-22:00 的文章发布时间回填到非工作时段。

默认 dry-run，只输出 old -> new 计划；必须显式传 ``--apply`` 才会调用
``POST /api/posts/{id}/timestamps``。脚本只写 published_at，不触碰正文、
浏览量、点赞数或 updated_at。
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import urllib.error
import urllib.parse
import urllib.request
from collections import defaultdict
from datetime import datetime, time
from pathlib import Path
from typing import Iterable
from zoneinfo import ZoneInfo


DEFAULT_TZ = "Asia/Shanghai"
WORK_START_HOUR = 10
WORK_END_HOUR = 22
BASE_SLOTS: list[tuple[int, int]] = [(0, 32), (1, 30), (22, 30), (23, 31)]
MAX_SLOTS_PER_DAY = 12


def parse_rfc3339(value: str) -> datetime:
    """解析 RFC3339，兼容 9 位小数秒（Python 3.10 fromisoformat 只吃 6 位）。"""
    match = re.match(r"^(.*?)(?:\.(\d+))?(Z|[+-]\d\d:\d\d)$", value)
    if not match:
        raise ValueError(f"非法 RFC3339 时间: {value}")
    base, fraction, zone = match.groups()
    normalized = base
    if fraction:
        normalized += "." + fraction[:6]
    normalized += "+00:00" if zone == "Z" else zone
    return datetime.fromisoformat(normalized)


def is_work_window(dt: datetime) -> bool:
    """工作日 10:00 <= 本地时间 < 22:00。"""
    return dt.weekday() < 5 and WORK_START_HOUR <= dt.hour < WORK_END_HOUR


def slots_for_count(count: int) -> list[tuple[int, int]]:
    """同一日期内按时间升序分配槽位，避免改完后顺序倒挂。"""
    if count <= 0:
        return []
    if count <= len(BASE_SLOTS):
        return BASE_SLOTS[:count]
    extra = count - len(BASE_SLOTS)
    if extra > 8:
        raise ValueError(f"单日最多支持 {MAX_SLOTS_PER_DAY} 篇文章，收到 {count}")
    early_extra = [(hour, 30) for hour in range(2, 2 + extra)]
    return [BASE_SLOTS[0], BASE_SLOTS[1], *early_extra, BASE_SLOTS[2], BASE_SLOTS[3]]


def plan_backfill(items: Iterable[dict], tz: ZoneInfo) -> list[dict]:
    """返回需要变更的 (id, old, new) 列表；周末和已非工作时段不改。"""
    grouped: dict[object, list[tuple[datetime, int]]] = defaultdict(list)
    for item in items:
        raw = item.get("published_at")
        if not raw:
            continue
        dt = parse_rfc3339(raw).astimezone(tz)
        if is_work_window(dt):
            grouped[dt.date()].append((dt, int(item["id"])))

    plan: list[dict] = []
    for day in sorted(grouped):
        same_day = sorted(grouped[day], key=lambda pair: (pair[0], pair[1]))
        slots = slots_for_count(len(same_day))
        for (old, post_id), (hour, minute) in zip(same_day, slots):
            new = datetime.combine(day, time(hour, minute), tzinfo=tz)
            plan.append({"id": post_id, "old": old, "new": new})
    return plan


def _request_json(
    method: str,
    url: str,
    token: str,
    *,
    payload: dict | None = None,
    timeout: int = 60,
) -> dict:
    headers = {"Accept": "application/json"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    data = None
    if payload is not None:
        data = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            body = resp.read()
    except urllib.error.HTTPError as exc:
        detail = exc.read().decode("utf-8", "replace")
        raise RuntimeError(f"HTTP {exc.code}: {detail[:300]}") from exc
    if not body:
        return {}
    return json.loads(body.decode("utf-8"))


def fetch_all_posts(base_url: str, token: str, timeout: int = 60) -> list[dict]:
    items: list[dict] = []
    page = 1
    while True:
        query = urllib.parse.urlencode({"page": page, "page_size": 100})
        payload = _request_json("GET", f"{base_url}/api/posts?{query}", token, timeout=timeout)
        data = payload.get("data", {})
        batch = data.get("items", [])
        items.extend(batch)
        total = int(data.get("total", len(items)))
        if not batch or len(items) >= total:
            return items
        page += 1


def fetch_site_timezone(base_url: str, token: str, timeout: int = 60) -> str:
    try:
        payload = _request_json("GET", f"{base_url}/api/settings", token, timeout=timeout)
        settings = payload.get("data", payload)
        if isinstance(settings, dict):
            value = settings.get("timezone")
            if isinstance(value, str) and value.strip():
                return value.strip()
    except Exception as exc:  # noqa: BLE001 - settings 失败时回落默认时区
        print(f"warning: 读取站点时区失败，回落 {DEFAULT_TZ}: {exc}", file=sys.stderr)
    return DEFAULT_TZ


def apply_plan(base_url: str, token: str, plan: list[dict], timeout: int = 60) -> int:
    applied = 0
    for change in plan:
        new_value = change["new"].isoformat(timespec="seconds")
        _request_json(
            "POST",
            f"{base_url}/api/posts/{change['id']}/timestamps",
            token,
            payload={"published_at": new_value},
            timeout=timeout,
        )
        applied += 1
    return applied


def _serialize_plan(plan: list[dict], tz_name: str) -> str:
    output = {
        "timezone": tz_name,
        "changed": len(plan),
        "items": [
            {
                "id": change["id"],
                "old": change["old"].isoformat(timespec="seconds"),
                "new": change["new"].isoformat(timespec="seconds"),
            }
            for change in plan
        ],
    }
    return json.dumps(output, ensure_ascii=False, indent=2)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-url", default=os.environ.get("HANCIC_BASE_URL", "http://127.0.0.1:8096"))
    parser.add_argument("--token", default=os.environ.get("HANCIC_API_TOKEN", ""))
    parser.add_argument("--timezone", default="", help="覆盖站点时区（默认从 /api/settings 读取）")
    parser.add_argument("--output", default="", help="审计 JSON 输出路径；缺省打印到 stdout")
    parser.add_argument("--apply", action="store_true", help="确认写回线上；缺省只 dry-run")
    parser.add_argument("--timeout", type=int, default=60)
    args = parser.parse_args(argv)

    base_url = args.base_url.rstrip("/")
    tz_name = args.timezone or fetch_site_timezone(base_url, args.token, args.timeout)
    try:
        tz = ZoneInfo(tz_name)
    except Exception as exc:  # noqa: BLE001
        print(f"错误：无效时区 {tz_name}: {exc}", file=sys.stderr)
        return 2

    try:
        items = fetch_all_posts(base_url, args.token, args.timeout)
        plan = plan_backfill(items, tz)
    except Exception as exc:  # noqa: BLE001
        print(f"错误：{exc}", file=sys.stderr)
        return 1

    audit = _serialize_plan(plan, tz_name)
    if args.output:
        Path(args.output).write_text(audit + "\n", encoding="utf-8")
        print(f"审计文件：{args.output}")
    else:
        print(audit)
    print(f"待回填 {len(plan)} 篇；模式={'apply' if args.apply else 'dry-run'}", file=sys.stderr)

    if not args.apply:
        return 0
    try:
        applied = apply_plan(base_url, args.token, plan, args.timeout)
    except Exception as exc:  # noqa: BLE001
        print(f"错误：写回失败，已处理 {applied if 'applied' in locals() else 0} 篇：{exc}", file=sys.stderr)
        return 1
    print(f"已回填 {applied} 篇", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
