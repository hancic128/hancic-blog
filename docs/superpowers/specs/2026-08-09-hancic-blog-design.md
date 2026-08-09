# Hancic 博客系统设计文档

- 日期：2026-08-09
- 状态：已批准（经 brainstorming 逐节确认）
- 项目代号：`hancic`（目录 `~/Project/hancic-blog`，暂定名，可改）
- 部署目标：上海云主机（172.81.241.149，4C8G），经北京 nginx 反代对外 `https://hancic.site/`

## 1. 背景与目标

现有博客基于 Halo 2.24（Java/Spring Boot，Docker 部署），内存占用大（500MB-1GB+），且主题定制受限于 Halo 引擎。目标：自研一套**轻量、可控、可复用**的博客系统，彻底替换 Halo。

核心诉求（用户原话收敛）：
- 资源占用少：CPU/内存/磁盘尽量小
- 支持 CMS 管理发布 + 博客长文 + 微博客（说说）双形态
- UI 简约现代优雅（Typora 式审美），主题可定制
- **移动端优先**：手机上写文章/发说说要顺手
- **AI 友好**：agent（Kimi Code / Claude Code 等）能直接通过 API 写博客、发说说、做管理
- 未来开源，让用户私有化部署，主题由社区贡献

## 2. 已确认决策（全部经用户确认）

| 项 | 决策 |
| --- | --- |
| 定位 | 公开品牌站 + 可复用轮子（未来开源、私有化部署） |
| 用户体系 | 单用户（无多用户/注册/角色） |
| 架构 | 单体 Rust 单二进制（SSR + 后台 + REST API + 静态资源） |
| 存储 | SQLite 嵌入（零独立数据库进程），FTS5 全文搜索 |
| 前台风格 | Typora 式文字流：大留白、正文 ~720px 行宽、无卡片、克制配色 |
| 说说形态 | 内嵌宫格（1 图大图 / 多图宫格 / 视频封面 / 文件卡片），按天分组（朋友圈式） |
| 说说互动 | v1 无互动；后期再议（评论走 giscus 或轻互动） |
| 后台编辑器 | Vditor 即时渲染模式（IR，Typora 交互），粘贴/拖拽即上传 |
| 文章功能 | 阅读量统计（必须）+ 全文搜索（v1 做）；RSS 不做；评论 v1 不做 |
| 后台模块 | 仪表盘 / 文章 / 说说 / 附件库 / 分类标签 / 站点设置 / 主题管理 / 统计 / API Token / 备份恢复 / 迁移导入 |
| 统计 | 阅读日志 + ip2region 离线地区解析（国家/省/市），总览/趋势/按文章/按地区 |
| 图片压缩 | 上传时自动压缩（限最长边 + 质量重编码，image crate） |
| 视频 | v1 不转码压缩，仅类型校验 + 大小上限（100MB）原样存储 |
| 技术栈 | Rust（axum + tera）+ sqlx（SQLite/FTS5）+ Vditor |
| 主题系统 | 文件系统主题：数据目录 `themes/<name>/`，theme.toml 契约，运行时加载 |
| AI 友好 | REST API + Bearer Token（后台生成/吊销）；MCP server 后期 |
| 部署 | 多阶段构建 distroless 镜像（~60MB）+ docker compose + GitHub Actions CI |
| 迁移 | 后台导入 halo-plugin-export-md 导出的 zip，批量建文章/分类/标签 + 图片下载选项 + 导入报告 |
| 测试 | 单元（模型/服务）+ API 集成（axum test）+ Playwright（含移动端视口） |
| 安全 | argon2 密码 / httpOnly Cookie + CSRF / Token 哈希存储 / 上传 mime+扩展名白名单 / sqlx 参数化 / Markdown 渲染默认安全 |

## 3. 架构总览

单 crate，按模块切分：

```
hancic（axum 应用）
├── config       # config.toml 加载（站点信息/主题/Token/上传限制）
├── db           # SQLite（sqlx：迁移 + FTS5 虚拟表 + 触发器）
├── models       # Post / Moment / Category / Tag / Attachment / Settings / ApiToken / PageView
├── web          # 前台 SSR（tera 渲染，走主题系统）
├── admin        # 后台页面（登录 + 11 个管理模块）
├── api          # REST API（Bearer Token 鉴权，AI 集成用）
├── themes       # 主题发现/加载/切换（运行时从磁盘加载）
├── upload       # 附件存储 + 图片压缩管线
├── stats        # 阅读日志写入 + ip2region 地区解析 + 统计查询
├── migrate      # Halo Markdown zip 导入
└── backup       # 全量导出/恢复（db + uploads + themes + config 打包 zip）
```

- 前台页面、后台页面、REST API 全部由同一 axum 进程提供
- 静态资源：主题 `static/` 目录（`/theme/<name>/static/...`）+ 上传附件（`/uploads/...`）由 ServeDir 服务，带缓存头
- 配置与数据全部位于数据目录（Docker 挂载卷，如 `/data`）

## 4. 数据模型

```
posts            # 文章
  id / slug(唯一) / title / content_md / excerpt / status(draft|published)
  published_at / created_at / updated_at / views
  category_id（单分类）／ ⇄ tags（多对多，经 post_tags）
  type = 'post' | 'page'（关于页等独立页面复用）
moments          # 说说
  id / content / created_at
  ⇄ attachments（有序多对多：图1→宫格位置）
categories       # 分类：id / slug / name / sort_order
tags             # 标签：id / slug / name
post_tags        # 文章-标签 关联
moment_attachments # 说说-附件 关联（含 sort_order）
attachments      # 附件：id / uuid_name / orig_name / mime / size / kind(image|video|file)
                  #  / path / created_at
settings         # 键值对：站点名/描述/导航/社交账号/active_theme 等
api_tokens       # API Token：id / token_hash / name / created_at / revoked_at
page_views       # 阅读日志：id / post_id / ip / ua / referer / region(国家|省|市) / created_at
posts_fts        # FTS5 虚拟表（title + content_md），触发器同步
```

- slug 唯一、URL 友好；slug 冲突自动加后缀
- `views` 简单计数（`+1`）；`page_views` 用于统计维度，后台可一键清理日志

## 5. 前台页面（SSR）

| 路由 | 内容 |
| --- | --- |
| `/` | 首页：文章流（分页，最新在前）+ 导航（分类/关于/说说入口） |
| `/post/{slug}` | 文章页：Typora 式正文 + 时间/分类/标签/阅读量 + 上一篇/下一篇 |
| `/moments` | 说说页：内嵌宫格 + 按天分组（朋友圈式时间线） |
| `/category/{slug}` | 分类归档 |
| `/tag/{slug}` | 标签归档 |
| `/search?q=` | 全文搜索（FTS5，关键词高亮） |
| `/about` | 关于页（type=page 内容） |
| 404 / 5xx | 错误页 |

移动端：导航折叠顶栏；文章页调整行宽/字号；图片懒加载 + lightbox；视频原生 `<video>` 播放；亮/暗色跟随系统 + 手动切换（主题变量驱动）。

## 6. 后台管理（`/admin`，登录后）

1. 仪表盘：文章/说说/附件数量、总阅读、近 30 日阅读趋势
2. 文章管理：列表（筛选：状态/分类/关键字）、新建/编辑（Vditor）、删除（含确认）
3. 说说管理：发布（朋友圈式输入框 + 选图/选视频）、列表、删除
4. 附件库：浏览（按类型筛选）、删除；图片自动压缩已在上传时完成
5. 分类标签：增删改、排序
6. 站点设置：站点名/描述/导航菜单/社交账号、亮暗色默认值、图片压缩开关、上传上限
7. 主题管理：列出已装主题、预览、切换（`active_theme` + 重启生效）
8. 统计模块：总览 / 近 N 日趋势 / 按文章排行 / 按地区（国家→省→市）/ 时间范围筛选 / 清理日志
9. API Token：生成（显示一次）/ 列表 / 吊销
10. 备份恢复：一键全量导出 zip / 上传恢复（含校验）
11. 迁移导入：上传 Halo zip → 选项（是否下载图片）→ 执行 → 导入报告

后台全局：响应式 + 触控优先；所有表单有验证与错误提示；危险操作二次确认。

## 7. 编辑体验与移动端

- 编辑器：Vditor IR 模式，桌面全功能 + 精简工具栏；移动端触控化按钮
- 自动保存草稿：编辑中每 30s + 失焦自动保存 draft，防丢失
- 说说发布框：朋友圈式（文字 + 选图/选视频），桌面端支持粘贴/拖拽
- 粘贴/拖拽上传：Vditor upload.handler 拦截 paste/drop → `POST /api/uploads` → 插入

## 8. REST API（AI 友好，Bearer Token）

```
POST   /api/posts                创建文章（Markdown 全文）
GET    /api/posts                列表（分页/筛选）
GET    /api/posts/{id}           详情
PATCH  /api/posts/{id}           更新（草稿/发布状态）
DELETE /api/posts/{id}           删除
POST   /api/moments              发说说（content + attachment ids）
DELETE /api/moments/{id}
POST   /api/uploads              附件上传（multipart，含图片压缩）
GET    /api/categories           分类列表
POST   /api/categories           新建分类
PATCH/DELETE /api/categories/{id}
GET    /api/stats/summary        统计摘要（AI 汇报用）
GET    /api/backup               全量导出
GET    /api/health               健康检查
```

- 统一 JSON：`{ "data": ... }` 成功 / `{ "error": { "code", "message" } }` 失败
- 附带一份《AI 集成指南》（skill/文档）：告诉 agent 各接口用法，注册进 Kimi/Claude
- MCP server 后期基于此 API 封装

## 9. 主题系统

```
<数据目录>/themes/<name>/
├── theme.toml    # name / author / version / description / 变量默认值（亮暗色、行宽等）
├── templates/
│   ├── index.html      # 首页
│   ├── post.html       # 文章页
│   ├── moments.html    # 说说页
│   ├── page.html       # 独立页（关于）
│   ├── category.html   # 分类/标签页
│   ├── search.html     # 搜索页
│   ├── partials/       # header / footer / 分页 / 卡片…
│   └── error.html      # 404/5xx
└── static/             # CSS/JS/图片，URL 前缀 /theme/<name>/static/
```

- 模板数据契约：context 固定暴露 `site`（设置）、`post`、`posts`、`moments`、`categories`、`page` 等，写《主题开发指南》文档
- 换主题 = 放置主题目录 + 改 `active_theme` + 重启（v1 不做热切换）
- 贡献主题 = 按契约写目录 + PR，不碰 Rust 代码
- 默认主题：Typora 式极简（大留白、无卡片、亮暗色），内嵌宫格与朋友圈时间线样式内置

## 10. 上传与媒体

- `POST /api/uploads`（multipart），按 `kind` 分目录：`uploads/image|video|file/`
- 校验：mime 白名单（image: jpg/png/webp/gif；video: mp4/webm/mov；file: 其他常见类型）+ 扩展名一致 + 大小上限（image 10MB / video 100MB / file 50MB）
- 图片压缩：最长边 2000px + 质量 85 重编码（image crate），可配置开关；GIF 只校验不压缩（动画）
- 文件名：UUID 重命名，防路径穿越
- 存储引用：数据库存相对路径，页面通过 `/uploads/{uuid}.{ext}` 访问

## 11. 阅读统计

- 前台文章访问时写入 `page_views`（post_id / ip / ua / referer / region / created_at）
- 地区解析：ip2region 离线 xdb（微秒级，零外部依赖），写入时同步解析国家/省/市
- 展示：总览（总量 + 趋势图）/ 按文章 / 按地区下钻 / 时间范围 / 一键清理日志
- 说明：IP 仅用于统计，自用合规；后台可清理

## 12. 迁移（Halo → hancic）

- 源：halo-plugin-export-md 导出的带 front-matter Markdown zip
- 流程：上传 zip → 解析 front-matter（title/date/categories/tags/slug）→ 批量建文章/分类/标签 → 可选下载正文图片到本地附件库（原站在线时）→ 导入报告（成功/失败/重复跳过）
- 说说：Halo 瞬间如可导出则半自动迁移；否则后台手动补录
- 迁移后验证：文章数、分类数、图片完整性抽查

## 13. 部署与 CI

- 多阶段构建：`rust:1.x` builder → `distroless`/`alpine` runner，镜像 ~60MB
- `docker-compose.yaml`：单服务，挂载数据卷（config/db/uploads/themes），健康检查，重启策略
- 升级：`docker compose pull && up -d`，数据全在卷内
- GitHub Actions：test + lint → 构建镜像 → 推送（ghcr/国内镜像）→ SSH 部署上海主机
- ⚠️ 约束：上海主机无法访问 GitHub；镜像拉取走国内加速或经可用主机中转（沿用现有 CI 矩阵经验：usa/bj 已接入，sh 作为部署目标）
- 备份：后台一键导出 zip（db+uploads+themes+config），恢复校验后可用
- 入口：上海主机新端口起服务，北京 nginx 根路径反代指向新端口；hancic.site 域名不变

## 14. 测试

- 单元：models/service 层（内存 SQLite + tokio test）
- 集成：axum test（REST API 全流程、上传/压缩、鉴权、统计）
- 前端：Playwright——文章发布全流程、说说发布（含选图）、粘贴上传、移动端视口（375px）核心路径
- CI 全量跑，lint + fmt + clippy

## 15. 安全基线

- 登录：argon2 密码哈希；session：httpOnly + Secure Cookie + CSRF token
- API Token：随机 32 字节，数据库存哈希，可吊销
- 上传：mime + 扩展名白名单、大小上限、UUID 命名、ServeDir 不执行脚本
- SQL：sqlx 参数化；XSS：tera 转义 + Markdown 渲染白名单（pulldown-cmark 默认安全）
- 速率限制：登录/API 简单限流（防爆破）

## 16. 里程碑与范围

**v1 范围（本设计）**：上述全部内容。

**明确不做（后期候选）**：
- 评论系统（后期 giscus/Gitalk）
- MCP server（后期基于 REST API 封装）
- 视频转码压缩、多用户、热切换主题、RSS/Atom、防刷统计、站点地图自动推送

## 17. 成功标准（验收）

1. 部署后空闲内存 ≤ 100MB（容器全部进程），镜像 ≤ 100MB
2. 移动端（375px 视口）可完成：发一条带图说说、写并发布一篇带图片文章
3. 桌面端粘贴截图进编辑器自动上传成功
4. Halo 导出 zip 导入后：文章/分类/标签数量一致，图片引用可用
5. agent 凭 Token 通过 API 成功创建并发布一篇文章
6. 后台统计按地区展示正确；图片上传后体积显著小于原图
7. `hancic.site` 切到新系统，功能与既有博客等价（文章可读、说说可看、导航可用）
