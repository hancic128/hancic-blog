# Changelog

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/) 与 [Semantic Versioning](https://semver.org/lang/zh-CN/)。

## [0.3.0] - 2026-09-06

### 新增（后台）

- 仪表盘：顶部统计卡两行自适应布局；文章排行固定 Top 10（去分页）；日期快捷新增「全部」（?range=all）；跳转来源改环形图；地区分布改**全球地图**（ECharts + 本地世界省界，按国家着色，随主题 accent/明暗联动）+ 明细列表分页
- 帮助文档页（侧栏新增菜单项）：集中 REST API 使用说明与 MCP 集成章节；Token 页原「接口使用说明」入口移除
- REST API 补齐：说说列表/详情/更新（部分更新）、附件库列表、标签创建（幂等）、站点设置只读、主题列表/切换、徒步轨迹列表/详情（?with_coords=1）
- MCP server 扩展至 **34 个工具**：新增说说读写、附件库、标签、设置、主题、轨迹 10 个工具，文档（后台帮助页 / mcp README / AI 集成指南）同步
- 后台默认主题色显式 emerald（未选择时 data-accent=emerald + 色板顺序）
- 悬浮按钮组交互与观感：hover 展开细化、移动端隐藏「折叠菜单」项、退出按钮实底（原亮色近透明）
- 附件库卡片 / 上传页 / 附件选择弹窗 UI 规范化（字号层级、间距、内边距）；主题管理卡片描述/作者固定高、按钮组贴底对齐；新建分类/标签/专栏统一为 Primary 按钮

### 新增（前台，default / modern / medium 三主题）

- 顶栏明暗/配色按钮收敛为右下**折叠式悬浮组**（与后台一致：hover 到主按钮展开、移出收起、展开动画、配色面板向左弹出）；联系卡片并入组内
- 回到顶部 / 阅读模式按钮保持独立悬浮，悬浮组展开时自动向上让位（保持 14px/12px 间距）

### 修复

- 阅读/点赞趋势与统计范围统一按**站点时区**自然日分组（此前 UTC 分组，M51 关闭；settings.timezone 配置）
- 前台配色面板弹出不再遮挡组内按钮；后台入口与控件按规范统一
- modern / medium 移动端抽屉导航导致的 375px 横向滚动
- 博客悬浮组 hover 误触发（折叠时触发区收窄为主按钮）
- trails E2E 断言与前台「点击卡片=选中聚焦」行为对齐（E2E 全绿 9/9）

### 新增（此前 Unreleased 内容随本次版本化）


- 首页「更新日历」→「发布日历」：只统计文章发布与说说，文章更新不再计入（三主题文案/样式同步）
- 专栏页默认按发布时间倒序（最新在前），显式 `?sort=` 仍走通用排序
- 阅读模式移动端内边距收窄；medium 主题代码块复制按钮样式补全

### 构建

- Dockerfile：`CARGO_SOURCE_INDEX` 镜像源覆盖前移至依赖层之前（镜像源可覆盖全部构建阶段，上海直连 crates.io sparse index 卡死时使用）
- `.dockerignore` 排除 `._*` / `.DS_Store`；`deploy-local.sh` 打包 `COPYFILE_DISABLE=1`——杜绝 macOS AppleDouble 元数据进镜像（8-28 线上 54 个 `._` 模板文件教训）

## [0.2.1] - 2026-08-28

### 新增

- 网站 favicon（default / medium / modern 三主题）
- 5 项体验优化
- medium 主题标签折叠按钮样式

### 修复

- 阅读按钮与联系按钮间距（按各主题 contact-fab 实际尺寸计算）
- 阅读模式保留联系方式卡片（仅隐藏返回顶部）；阅读图标移至联系图标上方 + hover 提示气泡
- 列表列宽 / 热力图占满容器宽度 / meta 对齐；medium 热力图占满容器宽度（对齐 default）

## [0.2.0] - 2026-08-28

### 新增

- **徒步轨迹功能**：后台轨迹管理（上传 GPX 自动解析/统计/DP 抽稀/编辑/删除）+ 前台 `/trails` 总览与 `/trails/{id}` 详情（Leaflet 本地化、高德卫星瓦片、排序/搜索）；导航新增「轨迹」类型
- **文章 URL 切换为 UUID**：前台文章链接 `/post/{uuid}`，旧 slug 链接 308 重定向（页面类型保留自定义 slug）
- 专栏管理优化：卡片折叠/展开 + 卡片间拖拽排序、专栏内文章拖拽排序、选择文章下拉自动向上弹出
- 站内搜索框、正文复制全文、env 语法高亮、热力图 tooltip 样式完善（08-26 线上批次还原）
- CI：GitHub Actions 仅打 tag（`v*`）时触发（分支 push/PR 不再消耗额度）；上海部署改后端直连 ghcr 拉镜像 + SSH 快速失败

### 修复

- modern 主题补齐排序条/专栏卡片/侧栏样式、两栏布局改正文居中 + 侧栏 fixed
- 移动端导航抽屉修复 + 轨迹徽标底色修正

## [0.1.5] - 2026-08-17

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

[Unreleased]: https://github.com/Angryshark128/hancic-blog/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/Angryshark128/hancic-blog/releases/tag/v0.2.1
[0.2.0]: https://github.com/Angryshark128/hancic-blog/releases/tag/v0.2.0
[0.1.5]: https://github.com/Angryshark128/hancic-blog/releases/tag/v0.1.5
[0.1.0]: https://github.com/Angryshark128/hancic-blog/releases/tag/v0.1.0
