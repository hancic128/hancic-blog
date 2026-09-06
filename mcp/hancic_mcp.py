#!/usr/bin/env python3
"""hancic MCP Server：把 hancic 博客的 REST API 包装为 MCP 工具，供
Claude Code / Codex / Kimi Code 等 AI 客户端调用。

鉴权：环境变量 HANCIC_API_TOKEN（后台「API Token」页生成，Bearer）。
地址：环境变量 HANCIC_BASE_URL，默认 http://127.0.0.1:8096。
  - 直连：http://127.0.0.1:8096（本地）或 https://hancic.site（生产）
  - 反向代理：填代理后的公开地址，如 https://blog.example.com；
    若反代有子路径（/blog → 本机 hancic），则填 https://example.com/blog。
API 固定挂在 <base>/api 下（hancic 内部 base_path 不影响 API 路由）。

运行：python hancic_mcp.py（stdio transport，供 MCP 客户端拉起）
"""

import os
import sys
import tempfile

import httpx
from mcp.server.fastmcp import FastMCP

BASE_URL = os.environ.get("HANCIC_BASE_URL", "http://127.0.0.1:8096").rstrip("/")
TOKEN = os.environ.get("HANCIC_API_TOKEN", "")

mcp = FastMCP("hancic")


class ApiError(Exception):
    """hancic API 错误（非 2xx）。"""


def _request(method: str, path: str, **kwargs) -> httpx.Response:
    headers = {"Authorization": f"Bearer {TOKEN}"} if TOKEN else {}
    headers.update(kwargs.pop("headers", {}) or {})
    try:
        resp = httpx.request(method, f"{BASE_URL}/api{path}", headers=headers, timeout=60, **kwargs)
    except httpx.HTTPError as e:
        raise ApiError(f"请求失败（{BASE_URL}）：{e}") from e
    if resp.status_code >= 400:
        try:
            err = resp.json().get("error", {}).get("message", resp.text)
        except Exception:
            err = resp.text[:200]
        raise ApiError(f"HTTP {resp.status_code}：{err}")
    if resp.status_code == 204:
        return resp
    return resp.json().get("data", resp.json())


# ---------------- 读 ----------------

@mcp.tool(description="文章列表（分页，可按状态/分类/标签筛选）")
def list_posts(
    page: int = 1,
    page_size: int = 10,
    status: str = "",
    category: str = "",
    tag: str = "",
) -> dict:
    params = {"page": page, "page_size": page_size}
    if status:
        params["status"] = status
    if category:
        params["category"] = category
    if tag:
        params["tag"] = tag
    return _request("GET", "/posts", params=params)


@mcp.tool(description="文章详情（含正文 Markdown）")
def get_post(post_id: int) -> dict:
    return _request("GET", f"/posts/{post_id}")


@mcp.tool(description="全量分类列表")
def list_categories() -> list:
    return _request("GET", "/categories")


@mcp.tool(description="阅读统计汇总（from_/to 为 YYYY-MM-DD，可选；对应 API 参数 from/to）")
def get_stats(from_: str = "", to: str = "") -> dict:
    params = {}
    if from_:
        params["from"] = from_
    if to:
        params["to"] = to
    return _request("GET", "/stats/summary", params=params)


@mcp.tool(description="下载全量备份 zip（数据库+配置+上传+主题），保存到本地并返回路径")
def get_backup(save_dir: str = "") -> dict:
    headers = {"Authorization": f"Bearer {TOKEN}"} if TOKEN else {}
    resp = httpx.get(f"{BASE_URL}/api/backup", headers=headers, timeout=120)
    if resp.status_code >= 400:
        raise ApiError(f"HTTP {resp.status_code}：备份失败")
    dest = os.path.join(save_dir or tempfile.gettempdir(), "hancic-backup.zip")
    with open(dest, "wb") as f:
        f.write(resp.content)
    return {"path": dest, "bytes": len(resp.content)}


@mcp.tool(description="服务健康检查（无需 token）")
def get_health() -> dict:
    return _request("GET", "/health")


# ---------------- 写 ----------------

@mcp.tool(description="创建文章（title/content_md 必填；status: draft|published，默认草稿）")
def create_post(
    title: str,
    content_md: str,
    status: str = "draft",
    excerpt: str = "",
    slug: str = "",
    category_id: int = 0,
    tags: list = None,
) -> dict:
    body = {"title": title, "content_md": content_md, "status": status}
    if excerpt:
        body["excerpt"] = excerpt
    if slug:
        body["slug"] = slug
    if category_id:
        body["category_id"] = category_id
    if tags:
        body["tags"] = tags
    return _request("POST", "/posts", json=body)


@mcp.tool(description="更新文章（PATCH：只传要改的字段；category_id=0 或 excerpt=\"\" 清空对应项）")
def update_post(
    post_id: int,
    title: str = "",
    content_md: str = "",
    status: str = "",
    excerpt: str = None,
    slug: str = "",
    category_id: int = None,
    tags: list = None,
) -> dict:
    body = {}
    if title:
        body["title"] = title
    if content_md:
        body["content_md"] = content_md
    if status:
        body["status"] = status
    if excerpt is not None:
        body["excerpt"] = excerpt if excerpt else None
    if slug:
        body["slug"] = slug
    if category_id is not None:
        body["category_id"] = category_id if category_id else None
    if tags is not None:
        body["tags"] = tags
    return _request("PATCH", f"/posts/{post_id}", json=body)


@mcp.tool(description="删除文章（不可恢复）")
def delete_post(post_id: int) -> dict:
    _request("DELETE", f"/posts/{post_id}")
    return {"ok": True, "post_id": post_id}


@mcp.tool(description="发布说说（content 非空；attachment_ids 为附件 id 数组，可空）")
def create_moment(content: str, attachment_ids: list = None) -> dict:
    body = {"content": content}
    if attachment_ids:
        body["attachment_ids"] = attachment_ids
    return _request("POST", "/moments", json=body)


@mcp.tool(description="删除说说（不可恢复）")
def delete_moment(moment_id: int) -> dict:
    _request("DELETE", f"/moments/{moment_id}")
    return {"ok": True, "moment_id": moment_id}


@mcp.tool(description="创建分类（name 必填，≤5 字；slug 缺省自动生成）")
def create_category(name: str, slug: str = "", sort_order: int = 0) -> dict:
    body = {"name": name, "sort_order": sort_order}
    if slug:
        body["slug"] = slug
    return _request("POST", "/categories", json=body)


@mcp.tool(description="更新分类（PATCH：只传要改的字段）")
def update_category(category_id: int, name: str = "", slug: str = "") -> dict:
    body = {}
    if name:
        body["name"] = name
    if slug:
        body["slug"] = slug
    return _request("PATCH", f"/categories/{category_id}", json=body)


@mcp.tool(description="删除分类（关联文章自动变为未分类）")
def delete_category(category_id: int) -> dict:
    _request("DELETE", f"/categories/{category_id}")
    return {"ok": True, "category_id": category_id}


@mcp.tool(description="全量标签列表（含各标签已发布文章数）")
def list_tags() -> list:
    return _request("GET", "/tags")


@mcp.tool(description="删除标签（关联文章不受影响，不可恢复）")
def delete_tag(tag_id: int) -> dict:
    _request("DELETE", f"/tags/{tag_id}")
    return {"ok": True, "tag_id": tag_id}


@mcp.tool(description="全量专栏列表（含各专栏已发布文章数）")
def list_columns() -> list:
    return _request("GET", "/columns")


@mcp.tool(description="创建专栏（name 必填，≤8 字；description ≤50 字；slug 缺省由名称自动生成）")
def create_column(name: str, slug: str = "", description: str = "", sort_order: int = 0) -> dict:
    body = {"name": name, "sort_order": sort_order}
    if slug:
        body["slug"] = slug
    if description:
        body["description"] = description
    return _request("POST", "/columns", json=body)


@mcp.tool(description="更新专栏名称/描述（PATCH：只传要改的字段；slug 创建后不变）")
def update_column(column_id: int, name: str = "", description: str = None) -> dict:
    body = {}
    if name:
        body["name"] = name
    if description is not None:
        body["description"] = description
    return _request("PATCH", f"/columns/{column_id}", json=body)


@mcp.tool(description="删除专栏（关联文章自动变为无专栏，不可恢复）")
def delete_column(column_id: int) -> dict:
    _request("DELETE", f"/columns/{column_id}")
    return {"ok": True, "column_id": column_id}


@mcp.tool(description="专栏下文章列表（分页，仅已发布，按更新时间倒序）")
def list_column_posts(column_id: int, page: int = 1, page_size: int = 10) -> dict:
    return _request("GET", f"/columns/{column_id}/posts", params={"page": page, "page_size": page_size})


@mcp.tool(description="把文章加入专栏（文章保留原分类/标签）")
def add_post_to_column(column_id: int, post_id: int) -> dict:
    return _request("POST", f"/columns/{column_id}/posts", json={"post_id": post_id})


@mcp.tool(description="把文章移出专栏（文章不删除）")
def remove_post_from_column(column_id: int, post_id: int) -> dict:
    _request("DELETE", f"/columns/{column_id}/posts/{post_id}")
    return {"ok": True, "column_id": column_id, "post_id": post_id}


@mcp.tool(description="上传附件（multipart，字段名 files）；返回附件 id/path，可用于说说 attachment_ids 或文章引用")
def upload_attachment(file_path: str) -> dict:
    if not os.path.isfile(file_path):
        raise ApiError(f"文件不存在：{file_path}")
    with open(file_path, "rb") as f:
        resp = httpx.post(
            f"{BASE_URL}/api/uploads",
            headers={"Authorization": f"Bearer {TOKEN}"} if TOKEN else {},
            files={"files": (os.path.basename(file_path), f)},
            timeout=120,
        )
    if resp.status_code >= 400:
        raise ApiError(f"HTTP {resp.status_code}：上传失败")
    return resp.json().get("data", resp.json())


# ---------------- 补充端点（2026-09-06 新增 REST API） ----------------

@mcp.tool(description="说说列表（分页；q 关键词 / month=YYYY-MM / order asc|desc）")
def list_moments(
    page: int = 1,
    page_size: int = 10,
    q: str = "",
    month: str = "",
    order: str = "",
) -> dict:
    params = {"page": page, "page_size": page_size}
    if q:
        params["q"] = q
    if month:
        params["month"] = month
    if order:
        params["order"] = order
    return _request("GET", "/moments", params=params)


@mcp.tool(description="说说详情（含附件数组）")
def get_moment(moment_id: int) -> dict:
    return _request("GET", f"/moments/{moment_id}")


@mcp.tool(description="更新说说（部分更新：content 非空才更新；attachment_ids 提供数组即整体替换，传 [] 清空附件）")
def update_moment(
    moment_id: int,
    content: str = "",
    attachment_ids: list = None,
) -> dict:
    payload = {}
    if content.strip():
        payload["content"] = content
    if attachment_ids is not None:
        payload["attachment_ids"] = attachment_ids
    if not payload:
        raise ApiError("至少提供一个字段：content 或 attachment_ids")
    return _request("PATCH", f"/moments/{moment_id}", json=payload)


@mcp.tool(description="附件库列表（kind=image/video/file；q 文件名关键词；order asc|desc；分页）")
def list_attachments(
    kind: str = "",
    q: str = "",
    order: str = "",
    page: int = 1,
    page_size: int = 20,
) -> dict:
    params = {"page": page, "page_size": page_size}
    if kind:
        params["kind"] = kind
    if q:
        params["q"] = q
    if order:
        params["order"] = order
    return _request("GET", "/attachments", params=params)


@mcp.tool(description="创建标签（name ≤5 字；同名幂等返回既有记录）")
def create_tag(name: str) -> dict:
    return _request("POST", "/tags", json={"name": name})


@mcp.tool(description="读取站点设置（全量键值，只读；写操作走后台）")
def get_settings() -> dict:
    return _request("GET", "/settings")


@mcp.tool(description="已安装主题列表（含 is_current 与当前主题）")
def list_themes() -> dict:
    return _request("GET", "/themes")


@mcp.tool(description="切换主题为当前（需重启服务后前台完全生效；404=主题不存在）")
def activate_theme(name: str) -> dict:
    return _request("POST", f"/themes/{name}/activate", json={})


@mcp.tool(description="徒步轨迹列表（里程/爬升/时长等统计概览）")
def list_trails() -> dict:
    return _request("GET", "/trails")


@mcp.tool(description="轨迹详情；with_coords=True 时附加完整坐标 [[lat, lon, speed], ...]")
def get_trail(trail_id: int, with_coords: bool = False) -> dict:
    params = {"with_coords": "1"} if with_coords else None
    return _request("GET", f"/trails/{trail_id}", params=params)


if __name__ == "__main__":
    if not TOKEN:
        print("警告：未设置 HANCIC_API_TOKEN，写操作将失败（只读工具可用）", file=sys.stderr)
    mcp.run(transport="stdio")
