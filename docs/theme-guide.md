# 主题指南（theme guide）

hancic 使用轻量主题系统：每个主题是 `themes/` 下的一个目录，包含 `theme.toml` 元信息、
`templates/` 模板（tera 2.x）与 `static/` 静态资源。当前内置默认主题 `themes/default`。

## 目录结构

```
themes/<name>/
├── theme.toml               # 主题元信息（必需）
├── templates/               # tera 模板（必需，glob 加载 templates/**/*.html）
│   ├── index.html           # 首页
│   ├── post.html            # 文章详情
│   ├── moments.html         # 动态页
│   ├── page.html            # 独立页面
│   ├── category.html        # 分类页
│   ├── search.html          # 搜索页
│   ├── error.html           # 错误页
│   └── partials/            # 可复用片段（header / footer / pagination）
└── static/                  # 静态资源（CSS / JS / 图片）
```

## theme.toml 字段

```toml
name = "default"          # 主题名（以目录名为准，theme.toml 中的值会被覆盖）
author = "hancic"         # 作者（可选，缺省空串）
version = "0.1.0"         # 版本号（可选，缺省空串）
description = "..."       # 描述（可选，缺省空串）
```

`theme.toml` 缺失或解析失败的主题会被 `themes::discover` 跳过并记录 warn 日志，
不会让博客启动失败。

## 模板契约（上下文变量）

渲染时注入的上下文：

| 模板 | 上下文 |
| --- | --- |
| `index.html` | `site`、`posts`、`pagination` |
| `post.html` | `site`、`post` |
| `moments.html` | `site`、`moments` |
| `page.html` | `site`、`page` |
| `category.html` | `site`、`categories` |
| `search.html` | `site`、`search_query`、`posts` |
| `error.html` | `site`、`error` |

- `site`：站点信息（`site_name`、`site_desc` 等，来自 settings）。
- `posts`：文章列表（`post.html` 同构对象数组：标题、摘要、日期、URL 等）。
- `pagination`：`{ page, total_pages, prev_url, next_url }`。
- 所有 partial 可被任意页面 `{% include "partials/header.html" %}` 引入。

## 过滤器

- `markdown`：把 Markdown 渲染为 HTML（已标记为安全，输出不参与自动转义）：
  `{{ post.content | markdown }}`
- `date`：把 RFC3339 时间字符串按站点时区格式化为 `YYYY-MM-DD HH:MM`：
  `{{ post.created_at | date }}`（时区当前固定 Asia/Shanghai，T17 起可配置）

## 静态资源

静态资源通过 URL 前缀 `/theme/<name>/static/` 访问：

```html
<link rel="stylesheet" href="/theme/default/static/style.css">
```

## 贡献新主题

- 在 `themes/` 下新建目录（如 `themes/my-theme/`），按上述结构补齐文件。
- 在 `config.toml` 中把 `active_theme` 改为主题目录名即可切换。
- **不需要修改任何 Rust 代码**；通过 PR 提交主题目录即可。
