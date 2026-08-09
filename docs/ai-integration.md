# Hancic REST API 集成指南（AI 专用）

本文档是给 AI 智能体（及未来 MCP server）对接 hancic 博客的接口手册。
所有端点基于 HTTP + JSON，鉴权用 API Token（Bearer）。

## 1. 概览

- **Base URL**：`https://<你的域名>/api`（本地开发 `http://127.0.0.1:8080/api`，端口以实际配置为准）
- **响应格式**：成功统一 `{"data": ...}`；失败统一 `{"error": {"code": <HTTP状态码>, "message": "<中文说明>"}}`
- **鉴权**：除 `/api/health` 外全部端点要求 `Authorization: Bearer <token>`（后台管理员会话也可，AI 一律用 Bearer）
- **时间**：所有时间字段为 UTC ISO 8601（如 `2026-08-09T02:00:00.000000000Z`）；`from`/`to` 类查询参数用 `YYYY-MM-DD`

### 状态码约定

| 状态码 | 含义 |
|---|---|
| 200 | 成功 |
| 201 | 创建成功（POST 类） |
| 204 | 删除成功（无响应体） |
| 400 | 请求体/参数校验失败（空标题、非法 status、分类不存在等） |
| 401 | 未携带或 Token 无效 |
| 404 | 资源不存在 |
| 409 | 冲突（如分类 slug 重复） |
| 500 | 服务端错误 |

## 2. 鉴权

1. 登录后台 `/admin/tokens` 生成 API Token，明文形如 `hc_xxxxxxxx...`，**仅显示一次**，请立即保存。
2. 请求头携带：`Authorization: Bearer hc_xxxxxxxx...`

```bash
curl -s -H "Authorization: Bearer $TOKEN" \
  https://example.com/api/posts
```

Token 无效或缺失返回：

```json
{"error": {"code": 401, "message": "无效的 API Token"}}
```

**错误处理建议**：收到 401 时检查 Token 是否已复制完整（46 字符，`hc_` 开头）或已在后台吊销；吊销后需重新生成。

## 3. 文章（Posts）

### 3.1 创建文章

```bash
curl -s -X POST https://example.com/api/posts \
  -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{
    "title": "我的第一篇 API 文章",
    "content_md": "# 标题\n\n正文用 **Markdown** 书写，原文会被完整保存。",
    "slug": "my-first-api-post",
    "excerpt": "正文用 Markdown 书写，原文会被完整保存。",
    "status": "draft",
    "category_id": 1,
    "tags": ["ai", "指南"]
  }'
```

响应 `201 Created`：

```json
{
  "data": {
    "id": 42,
    "slug": "my-first-api-post",
    "title": "我的第一篇 API 文章",
    "content_md": "# 标题\n\n正文用 **Markdown** 书写，原文会被完整保存。",
    "excerpt": "正文用 Markdown 书写，原文会被完整保存。",
    "status": "draft",
    "post_type": "post",
    "published_at": null,
    "created_at": "2026-08-09T02:00:00.000000000Z",
    "updated_at": "2026-08-09T02:00:00.000000000Z",
    "views": 0,
    "category_id": 1
  }
}
```

请求体字段：

| 字段 | 必填 | 说明 |
|---|---|---|
| `title` | ✅ | 非空字符串 |
| `content_md` | ✅ | Markdown 原文（可为空串，用于先建草稿） |
| `slug` | ❌ | 缺省由标题自动生成；重复时自动追加 `-2`、`-3`… |
| `excerpt` | ❌ | 摘要；缺省自动截取正文 |
| `status` | ❌ | `draft`（默认）或 `published`；发布即写 `published_at` |
| `category_id` | ❌ | 必须指向存在的分类，否则 400 |
| `tags` | ❌ | 字符串数组，自动建标签（同名复用） |

### 3.2 列表与筛选

```bash
curl -s -H "Authorization: Bearer $TOKEN" \
  "https://example.com/api/posts?page=1&page_size=10&status=published&category=tech&tag=ai"
```

- `page`：从 1 起，默认 1；`page_size`：默认 10，上限 100
- `status`：`draft` / `published`；`category` / `tag`：slug 精确匹配

响应 `{data: {items: [Post...], total: N}}`，Post 结构与 3.1 相同（含 `content_md` 原文与 `views`）。

### 3.3 文章详情

```bash
curl -s -H "Authorization: Bearer $TOKEN" https://example.com/api/posts/42
```

不存在返回 `404 {"error": {"code": 404, "message": "文章不存在"}}`。

### 3.4 更新文章（PATCH，部分字段）

```bash
curl -s -X PATCH https://example.com/api/posts/42 \
  -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"title": "改过的标题", "status": "published", "category_id": null}'
```

PATCH 语义：

- 缺失字段**保持不变**
- `"category_id": null` → **清空分类**；传整数则改分类（需存在）
- `"excerpt": ""` 或 `"excerpt": null` → **清空摘要**
- `"status": "published"` 会写入 `published_at`（草稿 → 发布时）

### 3.5 删除文章

```bash
curl -s -X DELETE -H "Authorization: Bearer $TOKEN" https://example.com/api/posts/42
```

成功 `204`（无响应体）；不存在 `404`。

## 4. 说说（Moments）

### 4.1 创建

```bash
curl -s -X POST https://example.com/api/moments \
  -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"content": "今天的碎碎念…", "attachment_ids": [3, 5]}'
```

`content` 必填非空；`attachment_ids` 可选整数数组（需指向已上传的附件，否则 400）。响应 `201 {data: {id, content, created_at}}`。

### 4.2 删除

```bash
curl -s -X DELETE -H "Authorization: Bearer $TOKEN" https://example.com/api/moments/7
```

成功 `204`；不存在 `404`。

## 5. 附件上传

```bash
curl -s -X POST https://example.com/api/uploads \
  -H "Authorization: Bearer $TOKEN" \
  -F "files=@/path/to/image.png"
```

- multipart 字段名固定为 `files`，可一次传多个文件
- 类型白名单：图片（jpeg/png/webp/gif）、视频（mp4/webm/mov）、文件（pdf/txt/zip/gz/bin/md）；超限或类型不符 400
- 响应 `{data: [Attachment...]}`，`Attachment` 含 `id`（后续写文章/说说时引用）、`url 相关 path` 等字段

## 6. 分类（Categories）

```bash
# 列表
curl -s -H "Authorization: Bearer $TOKEN" https://example.com/api/categories
# 创建
curl -s -X POST https://example.com/api/categories \
  -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"name": "技术", "slug": "tech", "sort_order": 1}'
# 更新（PATCH 部分字段）
curl -s -X PATCH https://example.com/api/categories/1 \
  -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"name": "工程技术"}'
# 删除
curl -s -X DELETE -H "Authorization: Bearer $TOKEN" https://example.com/api/categories/1
```

- 创建必填 `name`；`slug` 缺省由名称生成；重复 slug → 409
- 删除分类时关联文章的 `category_id` 自动置空（文章不删）

## 7. 统计

```bash
curl -s -H "Authorization: Bearer $TOKEN" \
  "https://example.com/api/stats/summary?from=2026-08-01&to=2026-08-09"
```

响应 `{data: {total_views, total_posts, total_moments, total_attachments, trend: [{date, count}...]}}`。`from`/`to` 可选（UTC 日期）。

## 8. 健康检查

```bash
curl -s https://example.com/api/health
# {"data": {"status": "ok"}}
```

不鉴权，供监控/部署探测。

## 9. 发布文章最佳实践（给 Agent 的推荐流程）

1. **先查再写**：`GET /api/categories` 确认分类存在（或先 `POST /api/categories` 建分类）；标签同名自动复用，无需预建。
2. **图片先传**：文章里的图片先 `POST /api/uploads` 拿到 `id`/URL，再写进 `content_md`。
3. **先草稿后发布**：长文建议先 `status: "draft"` 创建拿到 `id`，分多次 `PATCH` 完善，最后 `PATCH {"status": "published"}` 发布——避免半成品直接上线。
4. **校验返回**：每次写操作检查响应中的 `data.id`（201/200）确认成功；失败按 `error.code` 分类处理：
   - `400`：按 `message` 修正请求体；
   - `401`：Token 问题（见第 2 节）；
   - `409`：slug 冲突，换 slug 或去掉让服务端生成；
   - `5xx`：服务端异常，稍后重试并保留请求体。
5. **幂等注意**：POST 无幂等键，重复提交会重复建文章；如需保证只建一次，先 `GET /api/posts?page=1&page_size=1&status=draft` 核对或事后清理。

## 10. 将来 MCP 封装说明

后续 MCP server 将基于本文档实现，映射约定：

- 每个端点 → 一个 tool（`create_post`、`list_posts`、`get_post`、`update_post`、`delete_post`、`create_moment`、`delete_moment`、`upload_attachment`、`list_categories`、`create_category`、`update_category`、`delete_category`、`stats_summary`、`health`）
- 鉴权：Token 存于 MCP server 环境变量（如 `HANCIC_TOKEN`），所有请求统一注入 `Authorization` 头，不暴露给调用方
- 入参校验在 tool 层做（title 非空、status 枚举、category_id 存在性），把 400 提前转成 tool 参数错误，减少对服务端的无效请求
- 发布文章最佳实践（第 9 节）固化为一个组合 tool：`publish_article`（可传图 → 建草稿 → 补充字段 → 发布），对 AI 调用者提供"一步发布"体验
