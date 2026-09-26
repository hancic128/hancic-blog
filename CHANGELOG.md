# Changelog

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/) 与 [Semantic Versioning](https://semver.org/lang/zh-CN/)。

## [1.1.4] - 2026-09-26

### 新增

- **journal 主题源码入库**（`themes/journal/`）：此前该主题只以 zip 形式导入线上数据卷，仓库里没有源码——既无法在 CI/本地 E2E 回归，也无法随镜像内置。现与 default/medium/modern 同构入库，`entrypoint.sh` 启动时自动同步进数据卷（容器重建即到位，无需手动重导）。
- **后台品牌区最近部署时间**：站名下方新增小字「最近部署 YYYY-MM-DD HH:MM」。部署即容器重建即进程启动，故取 `AppState.started_at`，按「系统设置 / 时区」换算后展示；时区改动即时跟随。

### 变更

- **模板热重载**：`themes::ThemeTeraCache` 按主题名缓存 `Arc<Tera>`（读锁快路径 + 双重检查写锁），递归模板最大 mtime 判定失效；切主题、重新导入同名主题、卸载主题均即时生效，不再需要重启服务（后台提示语同步由「重启服务后完全生效」改为免重启）。
- **journal 双栏铺满**：≥1280px 两列由固定 `820px + 245px` 居中改为 `minmax(0, 1fr) 245px` 铺满 `.layout-two-col`，与分类/标签页版式一致，左右不再多留白；侧栏无内容时自动回退居中单列。正文列宽覆盖用 `:where()` 包裹保持 0-1-0 特异性，避免盖掉阅读模式的 46rem 限宽。
- **journal 右侧栏竖线样式**：右侧悬浮侧栏加 `border-left` 与左内边距，卡片背景/边框/圆角/阴影抹平，与左侧常驻侧栏同款；`.heatmap` 限宽 860px，`.site-footer` 不再限宽。
- **后台品牌区改为链接**：整块（logo + 站名）可点击，新标签页打开博客首页；hover/focus 只提亮文字不铺底色，focus-visible 加内描边；侧栏折叠时站名与部署时间一起隐藏。

### 测试

- `cargo test` 33/33 二进制全绿；`cargo clippy --all-targets -- -D warnings` 通过；Playwright E2E 17/17（desktop + 375px）。
- 新增 `tests/admin_flow.rs::admin_brand_deploy_time_follows_site_timezone`：系统时区改 UTC 后品牌区小字必须跟变，防写死 UTC+8。
- 新增 `e2e/tests/journal-layout.spec.ts`：双栏占满容器（左右贴边、列间距 <48px）、侧栏 `border-left: 1px`、阅读模式仍回 46rem 居中。
- 新增 `tests/admin_themes.rs` 断言：激活主题后下一次前台请求即渲染新主题模板（热生效，无须重启）。

## [1.1.3] - 2026-09-25

### 变更

- **后台字号全量令牌化（规范 02/10 收口）**：`assets/admin.css` 裸 px 字号清零。原 120 处 `font-size` 中仅 19 处引用令牌，现 114 处使用 `var(--text-xs/sm/base/lg/xl)`、6 处保留 em 相对值（编辑器标题与代码，相对排版）。其中 49 处为同值无损替换（12/14/16/18px）；45 处非标准值（13 / 13.5 / 12.5 / 11 / 15 / 17px）对齐最近令牌，单处视觉差 ≤ 1px。
- **新增 `--text-display: 27px`**：仅供仪表盘统计大数字（`.stat-num`）使用，是唯一允许超出 5 级字号体系（12/14/16/18/20px）的字号；令牌注释与 `docs/admin-design-tokens.md` 已标注该例外与防回潮约定。

### 测试

- `assets/admin.css` 字号全部走令牌，无裸 px 值；括号配平与改动前一致。
- `cargo test` 全绿、`cargo clippy --all-targets -- -D warnings` 通过、Playwright E2E 15/15（desktop + 375px）。

## [1.1.2] - 2026-09-23

### 新增

- **仪表盘累计口径**：日期范围仅控制阅读/点赞趋势；总阅读、总点赞、文章排行、地区分布、跳转来源全部改为累计口径。文章排行支持阅读量/点赞量切换。
- **地区分布国家汇总**：所有国家/地区按国家行汇总，省份明细保留在地图 tooltip 与表格展开项中；国内及港澳台统一归入中国。
- **时间戳回填脚本**：`scripts/backfill_published_at.py` 默认 dry-run，显式 `--apply` 才写回；只改 `published_at`，保留正文、浏览量、点赞数与 `updated_at`，并输出审计 JSON。

### 修复

- MCP `list_posts` 改为返回文章摘要，不再携带 `content_md`，避免文章列表响应被正文截断；取正文仍用 `get_post`。
- 后台侧栏折叠按钮移回侧栏底部；帮助页目录固定在视口右侧且不遮挡正文；移动端顶部菜单按钮改为靠右对齐。
- 文章编辑器链接按钮：有选区时为原文字套链接，无选区时追问链接显示文字；粘贴图片兼容 `clipboardData.items` 回退；上传接口返回 HTML 时显示可读错误，不再暴露 JSON 解析异常。

### 测试

- 新增仪表盘累计口径、排行切换、国家地区归并、来源计数一致性 Rust 集成测试。
- 新增链接、上传错误处理、侧栏折叠、帮助目录、移动端菜单对齐 Playwright E2E。
- 新增时间戳回填脚本单元测试与 MCP 列表摘要单元测试。

## [0.4.0] - 2026-09-16

### 新增

- **REST API 文章时间戳端口**（路线 v1.10 落地）：PATCH `/api/posts/{id}` 支持 `published_at` / `updated_at` RFC3339 字段（`published_at` 三态：缺失/null/RFC3339；`updated_at` 二态：缺失/RFC3339）；新增专用白名单端点 `POST /api/posts/{id}/timestamps` 用于事后回填到非工作时间窗口，至少传一字段否则 400。MCP `update_post` 加同名字段、新工具 `set_post_timestamps(post_id, published_at, updated_at, clear_published_at)`。`docs/ai-integration.md` 同步补 3.4 字段表 + 3.5 专用端点说明。
- **CI 发版 workflow 改造**（路线 v1.10b 落地）：`docker + deploy` job 从 push-tag 自动触发改为 `workflow_dispatch + inputs.tag` 手动触发；tag 在非工作窗口（工作日 08:00–09:30 / 22:30–01:30 或任意周末）由人在 Actions 页面 dispatch 输入，避免发版时间戳落在京东工作时间（10:00–22:00）。镜像 tag 跟随 `inputs.tag`，上海部署拉 `${{ inputs.tag }}` 而非 `latest`。

### 修复

- 帮助页目录格式：`admin.css` `.api-toc ul` 去掉 `display: flex; flex-wrap: wrap`（把目录项挤一行的根因），改为块级，每项一行；嵌套 `ul` 加 `padding-left: 1.25rem` 缩进表达层级。
- 前台右下悬浮按钮组：`#back-top` 从「垂直堆叠在主按钮上方」改为「与主按钮同水平、左侧间距 12px」（三主题 CSS + JS 同步），`layoutFloats` 移除 back-top 让位分支，只保留阅读模式按钮让位逻辑。fab 展开不再遮挡回顶按钮。

### 测试

- `tests/services_posts.rs` +5：自定义 `published_at` / 自定义 `updated_at` 跳过自动刷 / 不传仍自动刷 / 专用 `update_post_timestamps` / None 不写库。
- `tests/api_posts.rs` +2：PATCH 时间戳（含非法 400）+ 专用端点（成功/清空/空 body 400/非法 400/无 token 401）。
- `services_posts.rs::list_posts` 回归保险：`list_paginate_by_category_keeps_items_total_consistent` 覆盖「按分类筛选 + 分页 + content_md 不拼接」（P1-G 待办描述基于过期数据，加测试兜底）。
- 全量 `cargo test` 0 failed（含 Playwright E2E 9/9）。

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
