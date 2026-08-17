# Changelog

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/) 与 [Semantic Versioning](https://semver.org/lang/zh-CN/)。

## [Unreleased]

### 新增

- 专栏管理 REST API：GET/POST `/api/columns`、PATCH/DELETE `/api/columns/{id}`、GET/POST `/api/columns/{id}/posts`、DELETE `/api/columns/{id}/posts/{post_id}`（校验与后台一致：名称 ≤8 字、描述 ≤50 字）
- MCP Server 新增 7 个专栏工具：`list_columns` / `create_column` / `update_column` / `delete_column` / `list_column_posts` / `add_post_to_column` / `remove_post_from_column`
- 文章详情页 meta 两行布局：时间+统计行（发布/更新时间、阅读数、字数、阅读时长）与分类/专栏/标签行，分类与专栏带前缀
- 文章列表同时显示发布时间与更新时间
- 文章列表显示字数与预计阅读时长，排序新增「按阅读数」
- 默认导航种子内置「专栏」入口

### 修复

- 文章列表此前显示发布时间而按更新时间排序，现已显示更新时间，与排序一致
- 说说媒体链接缺失链接样式（默认蓝色下划线）
- 前台主题模板不同步导致 Docker 部署后回退旧模板

## [0.1.0] - 2026-08

首个可用版本。

### 新增

- **写作与内容**：Markdown（CommonMark + GFM），后台 milkdown 编辑器（严格标准、全屏、自动保存）；文章、独立页面、说说（支持图片/视频附件）
- **组织方式**：分类、标签、专栏（文章系列合集，前台卡片总览、侧栏导航、最热专栏）
- **前台**：更新日历热力图首页、最近说说时间线、最新文章；按更新时间/发布时间/阅读数排序；主题切换与配色切换；全文搜索（FTS5）；移动端响应式
- **后台管理**：仪表盘与阅读统计（含 IP 地域）、文章/说说/附件库、分类标签/专栏管理、站点设置（导航/社交/联系卡片/友情链接）、主题管理、系统设置、API Token、全量备份恢复、Halo 数据迁移
- **开放接口**：REST API 与 MCP Server 帮助页
- **工程化**：SQLite WAL 与幂等迁移、图片压缩、Docker（musl 静态编译）、GitHub Actions 测试/构建/自动部署、Playwright E2E

### 修复

- 后台移动端适配、配色面板超出视口
- 主题同步仅 default 的问题（改为同步全部内置主题）
- Clippy `-D warnings` 全绿

[Unreleased]: https://github.com/Angryshark128/hancic-blog/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Angryshark128/hancic-blog/releases/tag/v0.1.0
