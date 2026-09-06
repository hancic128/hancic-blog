# 寒蝉 Hancic

> 一个简洁、自托管的博客与内容发布系统。单二进制、零外部依赖，Markdown 写作，开箱即用。

[![CI](https://github.com/Angryshark128/hancic-blog/actions/workflows/ci.yaml/badge.svg)](https://github.com/Angryshark128/hancic-blog/actions/workflows/ci.yaml)

**在线预览**：<https://hancic.site/>

hancic（寒蝉）用 Rust 编写，以 SQLite 为存储，提供完整的博客能力：文章、说说（类似微博动态）、分类/标签/专栏、全文搜索、主题系统、后台管理与 REST API。后端一次编译为静态二进制，配合 Docker 可轻松部署到任意机器。

## ✨ 特性

- **写作**：Markdown（CommonMark + GFM 扩展），后台 milkdown 编辑器，支持严格 Markdown 标准、全屏写作、自动保存
- **内容形态**：文章、独立页面、说说（动态，支持图片/视频附件）
- **组织方式**：分类、标签、**专栏**（文章系列合集，前台卡片总览 + 侧栏导航）
- **前台体验**：
  - 首页更新日历热力图（GitHub Contribution 风格）、最近说说、最新文章
  - 按更新时间/发布时间/阅读数排序，列表显示字数与预计阅读时长
  - 文章详情显示发布/更新时间、所属分类/专栏/标签
  - 主题切换（亮/暗/跟随系统）与配色切换
  - 移动端响应式，适配手机浏览
  - 站点搜索（SQLite FTS5）
- **后台管理**（`/admin`）：
  - 仪表盘与阅读统计（含 IP 地域分布）
  - 文章/说说/附件库管理
  - 分类标签、专栏管理（拖拽式编排文章）
  - 站点设置（导航、社交链接、联系卡片、友情链接、Logo）
  - 主题管理（导入/卸载/切换）、系统设置
  - API Token、全量备份/恢复、**Halo 博客数据迁移**
- **开放接口**：REST API + MCP Server 帮助页（可被 AI 工具直接调用）
- **工程化**：SQLite WAL、启动自动迁移（幂等）、图片自动压缩、Docker 镜像（musl 静态编译）、GitHub Actions 自动测试与部署

## 🌱 部署默认值

首次启动（新数据目录）会自动初始化一组开箱即用的默认值，之后在后台随时修改：

| 项 | 默认 | 修改入口 |
|---|---|---|
| 站点名称 | `我的博客`（`config.toml` 的 `site_name` 仅在数据库尚无设置时兜底） | 后台「站点设置」 |
| 后台密码 | **无预设密码**，首次访问 `/admin/setup` 设置（≥8 位） | `/admin/setup` |
| 前台主题 | `default`（内置 default / modern / medium 三套） | 后台「主题管理」 |
| 导航 | 首页 / 文章 / 专栏 / 轨迹 / 说说 / 关于 | 后台「站点设置」可增删排序、行尾眼睛隐藏某项 |
| 时区 | `Asia/Shanghai`（阅读统计按该时区自然日分组） | 后台「系统设置」 |
| 运行配置 | 见 `config.example.toml`（端口 / 数据目录 / 上传限制等） | 复制为 `config.toml` 后按需修改 |

> 界面以在线预览 / 自行部署后实际体验为准，仓库不再附静态截图，保持文档轻量。
## 🚀 快速开始

### 本地构建运行

要求：Rust 1.88（见 `rust-toolchain.toml`）。

```bash
cargo build --release

# 首次运行：生成默认配置
cp config.example.toml config.toml

./target/release/hancic config.toml
```

打开 <http://localhost:8090> 即见站点首页；首次访问 `/admin/setup` 设置后台密码。

> **初始密码**：系统**没有预设默认密码**（不存在 admin/admin 之类的初始口令）。
> 首次运行必须访问 `/admin/setup` 自行设置后台密码（至少 8 位，Argon2 哈希存储），
> 之后通过 `/admin/login` 登录。若忘记密码，可删除数据表中 `admin_password_hash` 设置项后重新走 `/admin/setup`。

> 数据库文件位于配置的 `data_dir` 下（默认 `./data/hancic.db`），启动时自动创建并执行迁移，无需手动建表。

### Docker 部署

```bash
docker build -t hancic .
docker run -d --name hancic -p 8090:8090 -v "$(pwd)/data:/data" hancic
```

镜像已内置 `default` / `modern` / `medium` 三套主题，首次启动自动同步到数据目录。

### Docker Compose 部署

仓库附带了 `docker-compose.yaml`（默认本地构建 `hancic:latest`；也可自行推送镜像后替换 `image`）：

```bash
# 使用发布镜像（GitHub Container Registry 拉取）
docker compose up -d

# 或先本地构建再启动
docker compose build && docker compose up -d
```

`docker-compose.yaml` 要点：

- **数据持久化**：命名卷 `hancic-data` 挂载到容器 `/data`（数据库、上传文件、运行时主题都在其中），删除重建容器不丢数据
- **自定义配置**：如需覆盖默认配置，取消 `./config.toml:/app/config.toml:ro` 挂载注释即可（参考 `config.example.toml`）
- **健康检查**：内置 `/api/health` 探测，`docker compose ps` 可查看健康状态
- **资源限制**：默认限制 1 CPU / 256M 内存（静态博客足够；上传/备份时留足余量）
- **自动更新**：打 tag 发布后 CI 自动构建并推送新镜像，服务器上 `docker compose pull && docker compose up -d` 即可升级

> 生产部署/回滚（含镜像中转、健康轮询、回滚）请使用 `scripts/deploy-sh.sh`，详见 [docs/deploy-sh.md](docs/deploy-sh.md)。

## ⚙️ 配置

配置文件为 TOML，常用项见 `config.example.toml`：

| 字段 | 默认 | 说明 |
|---|---|---|
| `host` / `port` | `0.0.0.0` / `8090` | 监听地址 |
| `data_dir` | `/data` | 数据目录（数据库、上传、主题） |
| `base_path` | `""` | 部署子路径（如 `/blog`），配合反向代理 |
| `site_name` / `site_desc` | `我的博客`（仅 DB 未设置时兜底） | 站点名称与描述 |
| `active_theme` | `default` | 前台主题 |
| `image_compress` | `true` | 上传图片自动压缩 |
| `upload_max_*` | 10M / 100M / 50M | 图片/视频/文件上传上限 |

站点名称、导航、社交链接等运行时设置均在后台「设置」页维护（存储在数据库中）。

## 🎨 主题

主题系统基于 [Tera](https://keats.github.io/tera/) 模板。每个主题是 `themes/<name>/` 目录，包含 `theme.toml` 元信息、`templates/` 与 `static/`。

- 内置主题：`default`（经典简洁）、`modern`（现代卡片风）、`medium`（第三套自研）
- 后台「主题管理」支持在线导入（zip）/卸载/切换
- 开发新主题请参考 [docs/theme-guide.md](docs/theme-guide.md)

## 📚 文档

| 文档 | 说明 |
|---|---|
| [docs/theme-guide.md](docs/theme-guide.md) | 主题开发指南 |
| [docs/deploy-sh.md](docs/deploy-sh.md) | 生产部署/回滚手册 |
| [docs/ai-integration.md](docs/ai-integration.md) | REST API 与 MCP Server 集成说明 |

## 🗂 目录结构

```
├── src/                 # Rust 源码
│   ├── web/             # 前台渲染（front.rs）与路由
│   ├── admin/           # 后台管理模块
│   ├── api/             # REST API
│   ├── services/        # 业务服务（文章/说说/专栏/统计/备份/迁移…）
│   └── db.rs            # SQLite 连接与启动迁移
├── assets/              # 后台前端资源（模板 + 编辑器）
├── themes/              # 内置前台主题（镜像内嵌源）
├── data/themes/         # 运行时主题（entrypoint 同步）
├── migrations/          # SQL 迁移
├── tests/               # 集成测试
├── e2e/                 # Playwright 端到端测试
└── docs/                # 文档
```

## 🛠 开发

```bash
# 运行测试（单元 + 集成）
cargo test

# 静态检查（CI 以 -D warnings 兜底）
cargo clippy --all-targets -- -D warnings

# 端到端测试（Playwright）
cd e2e && npm ci && npx playwright test
```

开发新功能、提交规范请参阅 [CONTRIBUTING.md](CONTRIBUTING.md)。

## 🔒 安全

发现安全漏洞请勿公开讨论，参照 [SECURITY.md](SECURITY.md) 私密报告。

## 📄 许可证

[MIT](LICENSE) © 2026 Angryshark128
