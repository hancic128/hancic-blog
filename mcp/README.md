# hancic MCP Server

把 hancic 博客的 REST API 包装成 MCP 工具，供 Claude Code / Codex / Kimi Code 等
AI 客户端调用：查文章、发布文章、写说说、管理分类、上传附件、下载备份。

## 安装

```bash
cd mcp
python3 -m venv .venv && source .venv/bin/activate
pip install -r requirements.txt
```

## 配置（环境变量）

| 变量 | 说明 | 默认 |
|------|------|------|
| `HANCIC_API_TOKEN` | 后台「API Token」页生成的 token（Bearer 鉴权） | 必填（只读工具可不填） |
| `HANCIC_BASE_URL` | hancic 服务地址 | `http://127.0.0.1:8096` |

**反向代理场景**：`HANCIC_BASE_URL` 填代理后的公开地址。
hancic 的 API 固定在 `<base>/api` 下：

- 反代无子路径：`https://blog.example.com` → API 为 `https://blog.example.com/api/...`
- 反代有子路径（`/blog` 转发到 hancic）：`https://example.com/blog` → API 为 `https://example.com/blog/api/...`

## 接入 AI 客户端

### Claude Code

```bash
claude mcp add hancic -- \
  env HANCIC_API_TOKEN=你的token HANCIC_BASE_URL=https://hancic.site \
  python /path/to/hancic-blog/mcp/hancic_mcp.py
```

### Codex CLI

```bash
codex mcp add hancic -- \
  env HANCIC_API_TOKEN=你的token HANCIC_BASE_URL=https://hancic.site \
  python /path/to/hancic-blog/mcp/hancic_mcp.py
```

### Kimi Code / 其它

按客户端的 MCP 配置（stdio transport）注册同一命令，或写入项目级
`.mcp.json` / 客户端配置：

```json
{
  "mcpServers": {
    "hancic": {
      "command": "python",
      "args": ["/path/to/hancic-blog/mcp/hancic_mcp.py"],
      "env": {
        "HANCIC_API_TOKEN": "你的token",
        "HANCIC_BASE_URL": "https://hancic.site"
      }
    }
  }
}
```

## 工具清单

文章：`list_posts` / `get_post` / `create_post` / `update_post` / `delete_post`
说说：`list_moments` / `get_moment` / `create_moment` / `update_moment` / `delete_moment`
分类 / 标签：`list_categories` / `create_category` / `update_category` / `delete_category` /
`list_tags` / `create_tag` / `delete_tag`
专栏：`list_columns` / `create_column` / `update_column` / `delete_column` /
`list_column_posts` / `add_post_to_column` / `remove_post_from_column`
附件：`list_attachments` / `upload_attachment`
统计 / 设置 / 主题 / 轨迹 / 系统：`get_stats` / `get_settings` / `list_themes` /
`activate_theme` / `list_trails` / `get_trail` / `get_backup` / `get_health`

共 34 个工具。

> 注意：token 具备写权限（可删除内容/下载全量备份）。给 AI 用时注意保管；
> 如需只读，可在 hancic 后台吊销后按需生成，或等待后续版本支持 token 权限范围。
