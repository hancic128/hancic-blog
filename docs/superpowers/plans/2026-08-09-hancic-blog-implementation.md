# Hancic 博客系统实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 用 Rust 从零实现一个单用户、轻量、移动端优先的博客系统（文章 + 说说 + 后台 + REST API），替换上海主机上的 Halo。

**Architecture:** 单体 Rust 单二进制（axum SSR + 后台 + REST API + 静态资源），SQLite 嵌入存储（FTS5 全文搜索），主题为数据目录中的文件系统主题（theme.toml 契约），图片上传自动压缩，ip2region 离线地区统计，halo zip 迁移导入，alpine 多阶段构建 + docker compose 部署。

**Tech Stack:** Rust 1.88（edition 2024）、axum 0.8 + tera 1、sqlx 0.8（SQLite/FTS5）、tokio、pulldown-cmark、argon2、tower-sessions、image、ip2region、zip、reqwest、Vditor（IR 模式编辑器）、Chart.js、Playwright。

**参考规格:** `docs/superpowers/specs/2026-08-09-hancic-blog-design.md`（17 节，已批准）

## Global Constraints

以下约束来自设计文档，**所有任务隐式包含**，数值逐字照抄，不得放宽：

- 资源：部署后空闲内存 ≤ 100MB（容器全部进程），镜像 ≤ 100MB
- 移动端：375px 视口可完成"发一条带图说说、写并发布一篇带图片文章"
- 单用户：无注册、无多用户、无角色
- 数据目录：一切配置与数据位于数据目录（Docker 挂载卷，如 `/data`）；数据库路径 `<data>/hancic.db`，附件 `<data>/uploads/`，主题 `<data>/themes/`，配置 `<data>/config.toml`
- 上传白名单与上限：image `jpg/png/webp/gif`（≤10MB）；video `mp4/webm/mov`（≤100MB）；file 其他常见类型（≤50MB）
- 图片压缩：最长边 2000px + 质量 85 重编码（image crate），可配置开关；GIF 只校验不压缩
- 前台正文行宽 ~720px，Typora 式大留白、无卡片、亮暗色（跟随系统 + 手动切换）
- 后台 11 模块：仪表盘/文章/说说/附件库/分类标签/站点设置/主题管理/统计/API Token/备份恢复/迁移导入
- API 统一 JSON：成功 `{ "data": ... }`，失败 `{ "error": { "code", "message" } }`；鉴权 Bearer Token（`hc_` 前缀）
- 安全：argon2 密码哈希；httpOnly + Secure Cookie session；CSRF 校验；Token 存哈希；上传 mime+扩展名白名单；sqlx 参数化 SQL；Markdown 渲染默认安全（pulldown-cmark 默认）
- 迁移源：halo-plugin-export-md 导出的带 front-matter Markdown zip
- 语言：界面文案与文档为中文；代码/命令/提交信息为英文
- 主题契约：`<data>/themes/<name>/theme.toml`（name/author/version/description），模板 `templates/`，静态资源 `static/`；换主题=改 `active_theme` + 重启
- v1 明确不做：评论、MCP server、视频转码、多用户、主题热切换、RSS、防刷统计、站点地图

## 模块文件结构总览

最终目标文件树（括号内为创建该文件的任务，新增文件按任务添加）：

```
hancic-blog/
├── Cargo.toml                    (T1)
├── rust-toolchain.toml           (T1)
├── config.example.toml           (T1)
├── Dockerfile                    (T25)
├── docker-compose.yaml           (T25)
├── .github/workflows/ci.yaml     (T26)
├── scripts/
│   ├── fetch-assets.sh           (T12)  # 下载 Vditor/Chart.js 到 assets/vendor
│   └── deploy-sh.sh              (T27)  # 上海主机部署脚本
├── assets/vendor/                (T12)  # vditor.min.js/css、chart.umd.min.js
├── src/
│   ├── main.rs                   (T1)
│   ├── lib.rs                    (T1)   # pub fn app(...) 组装 Router
│   ├── config.rs                 (T1)
│   ├── error.rs                  (T1)   # AppError（API JSON 错误统一出口）
│   ├── db.rs                     (T2)   # 连接池 + 迁移 + 种子
│   ├── models.rs                 (T3)
│   ├── services/
│   │   ├── mod.rs                (T3)
│   │   ├── posts.rs              (T3)
│   │   ├── taxonomy.rs           (T3)
│   │   ├── settings.rs           (T4)
│   │   ├── moments.rs            (T11)
│   │   ├── uploads.rs            (T10)
│   │   ├── stats.rs              (T8)
│   │   ├── tokens.rs             (T20)
│   │   ├── backup.rs             (T22)
│   │   └── migrate.rs            (T23)
│   ├── auth.rs                   (T4)   # argon2 哈希/校验 + 首启设置
│   ├── session.rs                (T5)   # session store + 管理端中间件 + CSRF
│   ├── themes.rs                 (T6)   # 主题发现/加载/tera 构建
│   ├── markdown.rs               (T7)   # pulldown-cmark 渲染封装
│   ├── ipregion.rs               (T8)   # ip2region 封装
│   ├── web/
│   │   ├── mod.rs                (T7)
│   │   └── front.rs              (T7)   # 前台路由与 handlers
│   ├── admin/
│   │   ├── mod.rs                (T12)  # 后台路由树 + 布局 context
│   │   ├── posts.rs              (T13)
│   │   ├── moments.rs            (T14)
│   │   ├── attachments.rs        (T15)
│   │   ├── taxonomy.rs           (T16)
│   │   ├── settings.rs           (T17)
│   │   ├── themes.rs             (T18)
│   │   ├── stats.rs              (T19)
│   │   ├── tokens.rs             (T20)
│   │   └── backup.rs             (T22)
│   ├── api/
│   │   ├── mod.rs                (T10)  # /api 路由树
│   │   ├── auth.rs               (T21)  # Bearer 中间件
│   │   ├── posts.rs              (T21)
│   │   ├── moments.rs            (T21)
│   │   ├── uploads.rs            (T10)
│   │   ├── categories.rs         (T21)
│   │   ├── stats.rs              (T21)
│   │   └── backup.rs             (T21)
│   └── static/                   (T12)  # 后台自有 css/js（admin.css 等）
├── themes/default/               (T6)
│   ├── theme.toml                (T6)
│   ├── templates/…               (T7/T11)
│   └── static/…                  (T7)
├── migrations/
│   └── 001_init.sql              (T2)
├── tests/
│   ├── common/mod.rs             (T1)   # 测试辅助：临时数据目录 + 测试 app
│   ├── health.rs                 (T1)
│   ├── db_schema.rs              (T2)
│   ├── services_posts.rs         (T3)
│   ├── services_taxonomy.rs      (T3)
│   ├── auth_password.rs          (T4)
│   ├── session_csrf.rs           (T5)
│   ├── themes_load.rs            (T6)
│   ├── front_pages.rs            (T7)
│   ├── stats_region.rs           (T8)
│   ├── search_fts.rs             (T9)
│   ├── upload_compress.rs        (T10)
│   ├── moments_flow.rs           (T11)
│   ├── admin_flow.rs             (T12)
│   ├── admin_posts.rs            (T13)
│   ├── admin_moments.rs          (T14)
│   ├── admin_attachments.rs      (T15)
│   ├── admin_taxonomy.rs         (T16)
│   ├── admin_settings.rs         (T17)
│   ├── admin_themes.rs           (T18)
│   ├── admin_stats.rs            (T19)
│   ├── admin_tokens.rs           (T20)
│   ├── api_posts.rs              (T21)
│   ├── api_auth.rs               (T21)
│   ├── backup_restore.rs         (T22)
│   ├── migrate_halo.rs           (T23)
├── e2e/                          (T24)
│   ├── playwright.config.ts      (T24)
│   ├── package.json              (T24)
│   └── tests/…                   (T24)
└── docs/
    ├── ai-integration.md         (T21)
    └── theme-guide.md            (T6)
```

**测试约定**：所有 axum 集成测试用 `tests/common/mod.rs` 的 `test_app()`（内存或临时目录 SQLite + 默认主题），`tower::ServiceExt::oneshot` 发请求。单元测试放各自 `src/` 模块内 `#[cfg(test)]`。所有服务函数接受 `&Db`（`sqlx::SqlitePool`）作为第一参数，便于测试。

---

## 阶段 A：地基（T1–T3）

### Task 1: 项目脚手架、配置加载、健康检查

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `.gitignore`（已存在，追加），`config.example.toml`, `src/main.rs`, `src/lib.rs`, `src/config.rs`, `src/error.rs`, `tests/common/mod.rs`, `tests/health.rs`

**Interfaces:**
- Consumes: 无（项目为空）
- Produces:
  - `Config { host: String, port: u16, data_dir: PathBuf, site_name: String, site_desc: String, active_theme: String, image_compress: bool, image_max_edge: u32, image_quality: u8, upload_max_image: u64, upload_max_video: u64, upload_max_file: u64 }`，`config::load(path: &Path) -> Result<Config>`（文件不存在时用默认值）
  - `AppError`（`impl IntoResponse`，API 错误统一 JSON；内部错误日志后返回 500）
  - `hancic::app(config: Config) -> Router`（组装路由，此时只挂 `/api/health`）
  - `tests/common::test_app() -> Router`：用临时目录 + 默认配置构建 app（后续任务复用）

- [ ] **Step 1: 初始化 Cargo 项目与工具链**

```bash
cd ~/Project/hancic-blog
cargo init --name hancic --vcs none
echo '1.88.0' > rust-toolchain.toml   # rust-toolchain.toml 内容为 "1.88.0"
cargo add axum --features multipart
cargo add tokio --features full
cargo add tower tower-http --features "fs,cors,limit,trace"
cargo add serde --features derive
cargo add serde_json
cargo add sqlx --features "sqlite,runtime-tokio,time"
cargo add tera
cargo add pulldown-cmark --no-default-features --features html
cargo add argon2
cargo add tower-sessions --features sqlite-store
cargo add chrono serde
cargo add uuid --features v4
cargo add ip2region
cargo add image --features "jpeg,png,webp,gif"
cargo add zip --features deflate
cargo add tracing tracing-subscriber
cargo add rand
cargo add reqwest --features json
cargo add sha2
cargo add base64
```

若 `tower-sessions` 当前最新版本无 `sqlite-store` feature（版本 API 变动），改用其推荐 store crate 并在此任务记录实际 feature 名，后续任务沿用。

- [ ] **Step 2: 写配置加载与健康检查的失败测试**

`tests/health.rs`:

```rust
use hancic::config::Config;
use tower::ServiceExt;

#[tokio::test]
async fn health_returns_ok() {
    let cfg = Config::default_for_temp_dir();
    let app = hancic::app(cfg);
    let res = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/health")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);
}
```

`tests/common/mod.rs`:

```rust
use hancic::config::Config;
use std::path::PathBuf;

pub fn temp_data_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hancic-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn test_config(tag: &str) -> Config {
    Config::default_for_temp_dir().with_data_dir(temp_data_dir(tag))
}

pub async fn test_app(tag: &str) -> axum::Router {
    hancic::app(test_config(tag))
}
```

- [ ] **Step 3: 运行测试确认失败**

Run: `cargo test --test health`
Expected: 编译失败（`hancic` 与 `Config` 不存在）

- [ ] **Step 4: 实现 config、error、app 骨架**

`src/config.rs`:

```rust
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub data_dir: PathBuf,
    pub site_name: String,
    pub site_desc: String,
    pub active_theme: String,
    pub image_compress: bool,
    pub image_max_edge: u32,
    pub image_quality: u8,
    pub upload_max_image: u64,
    pub upload_max_video: u64,
    pub upload_max_file: u64,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, config_error::LoadError> {
        // 文件不存在时全部用默认值；存在时用 toml 覆盖（serde 缺省字段取默认）
        let default = Self::defaults();
        match std::fs::read_to_string(path) {
            Ok(content) => Ok(toml::from_str::<Config>(&content)
                .map_err(|e| config_error::LoadError::Parse(e.to_string()))?
                .merge(default)),
            Err(_) => Ok(default),
        }
    }
}
```

为保持简单，`Config::load` 的具体实现采用"默认值 + 逐字段覆盖"的合并函数（`merge`），此处不引入外部 config crate；`config_error::LoadError` 简化为 `String` 错误。`Config` 增加方法：

```rust
impl Config {
    pub fn defaults() -> Self { /* host 0.0.0.0, port 8090, data_dir ./data,
        site_name "寒蝉 Hancic", site_desc "", active_theme "default",
        image_compress true, image_max_edge 2000, image_quality 85,
        upload_max_image 10MB, upload_max_video 100MB, upload_max_file 50MB */ }
    pub fn default_for_temp_dir() -> Self { /* defaults() + data_dir = env::temp_dir()/hancic-dev */ }
    pub fn with_data_dir(mut self, dir: PathBuf) -> Self { self.data_dir = dir; self }
}
```

`src/error.rs`：

```rust
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

pub enum AppError {
    NotFound(String),
    BadRequest(String),
    Unauthorized(String),
    Forbidden(String),
    Internal(String),
    Conflict(String),
}

impl AppError {
    fn status(&self) -> StatusCode {
        match self {
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            AppError::Forbidden(_) => StatusCode::FORBIDDEN,
            AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::Conflict(_) => StatusCode::CONFLICT,
        }
    }
    fn message(&self) -> &str {
        match self {
            AppError::NotFound(m) | AppError::BadRequest(m) | AppError::Unauthorized(m)
            | AppError::Forbidden(m) | AppError::Internal(m) | AppError::Conflict(m) => m,
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status();
        let body = Json(json!({
            "error": { "code": status.as_u16(), "message": self.message() }
        }));
        (status, body).into_response()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        tracing::error!("sqlx error: {e}");
        AppError::Internal("数据库错误".into())
    }
}

pub type AppResult<T> = Result<T, AppError>;
```

`src/lib.rs`：

```rust
pub mod config;
pub mod db;
pub mod error;

use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;
use crate::config::Config;

pub fn app(config: Config) -> Router {
    let state = AppState { config: Arc::new(config) };
    Router::new()
        .route("/api/health", get(health))
        .with_state(state)
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
}

async fn health() -> Json<Value> {
    Json(json!({ "data": { "status": "ok" } }))
}
```

`src/main.rs`：

```rust
use hancic::config::Config;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter(
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "info,hancic=debug".into()),
    ).init();

    let config_path = std::env::args().nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/config.toml"));
    let config = Config::load(&config_path)?;
    let addr = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("hancic listening on {addr}");
    axum::serve(listener, hancic::app(config)).await?;
    Ok(())
}
```

`config.example.toml`（提交到仓库，作为部署模板）：

```toml
host = "0.0.0.0"
port = 8090
data_dir = "/data"
site_name = "寒蝉 Hancic"
site_desc = "记录与思考"
active_theme = "default"
image_compress = true
image_max_edge = 2000
image_quality = 85
upload_max_image = 10485760
upload_max_video = 104857600
upload_max_file = 52428800
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test --test health`
Expected: PASS；`cargo run` 后 `curl -s http://127.0.0.1:8090/api/health` 返回 `{"data":{"status":"ok"}}`

- [ ] **Step 6: 提交**

```bash
git add -A && git commit -m "feat: 项目脚手架、配置加载与健康检查"
```

---

### Task 2: 数据库 schema 迁移与连接池

**Files:**
- Create: `migrations/001_init.sql`, `src/db.rs`
- Test: `tests/db_schema.rs`

**Interfaces:**
- Consumes: `Config`（T1）
- Produces:
  - `db::init(data_dir: &Path) -> Result<sqlx::SqlitePool>`：建 `data_dir` 目录、`PRAGMA journal_mode=WAL; busy_timeout=5000; foreign_keys=ON`、执行内嵌迁移、写入默认 settings 种子
  - `type Db = sqlx::SqlitePool`（`src/db.rs` 中 `pub use`）

- [ ] **Step 1: 写 schema 迁移 SQL（完整覆盖 §4 数据模型）**

`migrations/001_init.sql`：

```sql
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS categories (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  slug       TEXT NOT NULL UNIQUE,
  name       TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS tags (
  id   INTEGER PRIMARY KEY AUTOINCREMENT,
  slug TEXT NOT NULL UNIQUE,
  name TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS attachments (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  uuid_name  TEXT NOT NULL UNIQUE,
  orig_name  TEXT NOT NULL,
  mime       TEXT NOT NULL,
  size       INTEGER NOT NULL,
  kind       TEXT NOT NULL CHECK (kind IN ('image','video','file')),
  path       TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);

CREATE TABLE IF NOT EXISTS posts (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  slug         TEXT NOT NULL UNIQUE,
  title        TEXT NOT NULL,
  content_md   TEXT NOT NULL DEFAULT '',
  excerpt      TEXT NOT NULL DEFAULT '',
  status       TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft','published')),
  post_type    TEXT NOT NULL DEFAULT 'post' CHECK (post_type IN ('post','page')),
  published_at TEXT,
  created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  updated_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  views        INTEGER NOT NULL DEFAULT 0,
  category_id  INTEGER REFERENCES categories(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS moments (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  content    TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);

CREATE TABLE IF NOT EXISTS post_tags (
  post_id INTEGER NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
  tag_id  INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  PRIMARY KEY (post_id, tag_id)
);

CREATE TABLE IF NOT EXISTS moment_attachments (
  moment_id     INTEGER NOT NULL REFERENCES moments(id) ON DELETE CASCADE,
  attachment_id INTEGER NOT NULL REFERENCES attachments(id) ON DELETE CASCADE,
  sort_order    INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (moment_id, attachment_id)
);

CREATE TABLE IF NOT EXISTS settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS api_tokens (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  token_hash TEXT NOT NULL UNIQUE,
  name       TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  revoked_at TEXT
);

CREATE TABLE IF NOT EXISTS page_views (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  post_id    INTEGER NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
  ip         TEXT NOT NULL DEFAULT '',
  ua         TEXT NOT NULL DEFAULT '',
  referer    TEXT NOT NULL DEFAULT '',
  country    TEXT NOT NULL DEFAULT '',
  province   TEXT NOT NULL DEFAULT '',
  city       TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);

CREATE INDEX IF NOT EXISTS idx_posts_status_published ON posts(status, published_at DESC);
CREATE INDEX IF NOT EXISTS idx_posts_slug ON posts(slug);
CREATE INDEX IF NOT EXISTS idx_moments_created ON moments(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_page_views_post ON page_views(post_id);
CREATE INDEX IF NOT EXISTS idx_page_views_created ON page_views(created_at);

CREATE VIRTUAL TABLE IF NOT EXISTS posts_fts USING fts5(
  title, content_md,
  content='posts', content_rowid='id', tokenize='unicode61'
);

CREATE TRIGGER IF NOT EXISTS posts_ai AFTER INSERT ON posts BEGIN
  INSERT INTO posts_fts(rowid, title, content_md) VALUES (new.id, new.title, new.content_md);
END;
CREATE TRIGGER IF NOT EXISTS posts_ad AFTER DELETE ON posts BEGIN
  INSERT INTO posts_fts(posts_fts, rowid, title, content_md) VALUES('delete', old.id, old.title, old.content_md);
END;
CREATE TRIGGER IF NOT EXISTS posts_au AFTER UPDATE ON posts BEGIN
  INSERT INTO posts_fts(posts_fts, rowid, title, content_md) VALUES('delete', old.id, old.title, old.content_md);
  INSERT INTO posts_fts(rowid, title, content_md) VALUES (new.id, new.title, new.content_md);
END;
```

（`posts.category_id` 引用 `categories`，故建表顺序把 `categories`、`tags`、`attachments` 置于 `posts` 之前，见上方已调整。）

- [ ] **Step 2: 写 db 初始化失败测试**

`tests/db_schema.rs`：

```rust
mod common;
use hancic::db;
use hancic::config::Config;

#[tokio::test]
async fn schema_creates_all_tables() {
    let cfg = Config::default_for_temp_dir().with_data_dir(common::temp_data_dir("schema"));
    let pool = db::init(&cfg.data_dir).await.unwrap();
    let names: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"
    ).fetch_all(&pool).await.unwrap().into_iter().map(|s| s.unwrap()).collect();
    for t in ["posts","moments","categories","tags","post_tags","moment_attachments",
              "attachments","settings","api_tokens","page_views"] {
        assert!(names.iter().any(|n| n == t), "缺少表 {t}");
    }
    let fts: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type='table' AND name='posts_fts'"
    ).fetch_all(&pool).await.unwrap().into_iter().map(|s| s.unwrap()).collect();
    assert_eq!(fts.len(), 1);
}
```

- [ ] **Step 3: 运行测试确认失败**

Run: `cargo test --test db_schema`
Expected: 编译失败（`db` 模块不存在）

- [ ] **Step 4: 实现 db 初始化**

`src/db.rs`：

```rust
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{ConnectOptions, SqlitePool};
use std::path::Path;
use std::str::FromStr;

pub type Db = SqlitePool;

const MIGRATION_001: &str = include_str!("../migrations/001_init.sql");

pub async fn init(data_dir: &Path) -> Result<Db, sqlx::Error> {
    std::fs::create_dir_all(data_dir).map_err(sqlx::Error::Configuration)?;
    let db_path = data_dir.join("hancic.db");
    let opts = SqliteConnectOptions::from_str(&format!("sqlite://{}", db_path.display()))?
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(std::time::Duration::from_secs(5))
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(10)
        .connect_with(opts).await?;
    sqlx::raw_sql(MIGRATION_001).execute(&pool).await?;
    seed_default_settings(&pool).await?;
    Ok(pool)
}

async fn seed_default_settings(pool: &Db) -> Result<(), sqlx::Error> {
    let defaults: &[(&str, &str)] = &[
        ("site_name", "寒蝉 Hancic"),
        ("site_desc", ""),
        ("site_nav", r#"[{"label":"首页","url":"/"},{"label":"说说","url":"/moments"},{"label":"关于","url":"/about"}]"#),
        ("site_social", r#"{}"#),
        ("active_theme", "default"),
        ("theme_mode", "auto"),        // auto | light | dark
        ("timezone", "Asia/Shanghai"),
    ];
    for (k, v) in defaults {
        sqlx::query("INSERT OR IGNORE INTO settings(key, value) VALUES (?, ?)")
            .bind(k).bind(v).execute(pool).await?;
    }
    Ok(())
}
```

**接口修正**：`hancic::app` 为 `pub async fn app(config: Config) -> Router`（内部 `db::init`）。T1 已提交的 `tests/health.rs` 与 `tests/common/mod.rs` 同步更新为 async 调用（`test_app` 改为 `pub async fn test_app(tag: &str) -> Router`，内部 `hancic::app(test_config(tag)).await`）。

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test --test db_schema`
Expected: PASS（FTS5 表存在、10 张业务表存在）

- [ ] **Step 6: 提交**

```bash
git add -A && git commit -m "feat: SQLite schema、FTS5 与连接池初始化"
```

---

### Task 3: 核心模型与文章/分类/标签服务层

**Files:**
- Create: `src/models.rs`, `src/services/mod.rs`, `src/services/posts.rs`, `src/services/taxonomy.rs`
- Test: `tests/services_posts.rs`, `tests/services_taxonomy.rs`

**Interfaces:**
- Consumes: `Db`（T2）、`Config`（T1）
- Produces（本任务起所有服务函数签名固定，后续任务按此调用）:

```rust
// models.rs
pub enum PostStatus { Draft, Published }   // serde rename: "draft"/"published"
pub enum PostType { Post, Page }           // serde rename: "post"/"page"
pub enum AttachmentKind { Image, Video, File }
pub struct Post { id, slug, title, content_md, excerpt, status: PostStatus,
    post_type: PostType, published_at: Option<DateTime<Utc>>, created_at, updated_at,
    views: i64, category_id: Option<i64> }
pub struct Moment { id, content, created_at }
pub struct Category { id, slug, name, sort_order }
pub struct Tag { id, slug, name }
pub struct Attachment { id, uuid_name, orig_name, mime, size: i64, kind: AttachmentKind,
    path, created_at }
pub struct ApiToken { id, token_hash, name, created_at, revoked_at: Option<String> }
pub struct PageView { id, post_id, ip, ua, referer, country, province, city, created_at }

// services/posts.rs
pub struct NewPost { pub title: String, pub content_md: String, pub excerpt: Option<String>,
    pub slug: Option<String>, pub status: PostStatus, pub post_type: PostType,
    pub category_id: Option<i64>, pub tags: Vec<String> }  // tags 为标签名列表
pub struct UpdatePost { /* 所有字段 Option<...>，None=不变；slug/tags 特殊：Some(_) 即替换 */ }
pub struct PostListOptions { pub status: Option<PostStatus>, pub category_slug: Option<String>,
    pub tag_slug: Option<String>, pub page: i64, pub page_size: i64 }

pub async fn slugify(input: &str) -> String
pub async fn create_post(db: &Db, input: NewPost) -> Result<Post, AppError>
pub async fn get_post(db: &Db, id: i64) -> Result<Option<Post>, AppError>
pub async fn get_post_by_slug(db: &Db, slug: &str) -> Result<Option<Post>, AppError>
pub async fn list_posts(db: &Db, opts: PostListOptions) -> Result<(Vec<Post>, i64), AppError>
pub async fn update_post(db: &Db, id: i64, input: UpdatePost) -> Result<Post, AppError>
pub async fn delete_post(db: &Db, id: i64) -> Result<(), AppError>
pub async fn increment_views(db: &Db, id: i64) -> Result<(), AppError>
pub async fn adjacent_posts(db: &Db, p: &Post) -> Result<(Option<Post>, Option<Post>), AppError>
pub async fn count_posts(db: &Db) -> Result<i64, AppError>
pub async fn list_tags_of_post(db: &Db, post_id: i64) -> Result<Vec<Tag>, AppError>
pub async fn set_post_tags(db: &Db, post_id: i64, tags: &[String]) -> Result<(), AppError>
pub async fn excerpt_of(md: &str) -> String   // 纯函数：去 markdown 标记取前 150 字

// services/taxonomy.rs
pub async fn list_categories(db: &Db) -> Result<Vec<Category>, AppError>
pub async fn create_category(db: &Db, name: &str, slug: &str, sort_order: i64) -> Result<Category, AppError>
pub async fn update_category(db: &Db, id: i64, name: &str, slug: &str, sort_order: i64) -> Result<Category, AppError>
pub async fn delete_category(db: &Db, id: i64) -> Result<(), AppError>
pub async fn get_category_by_slug(db: &Db, slug: &str) -> Result<Option<Category>, AppError>
pub async fn list_tags(db: &Db) -> Result<Vec<Tag>, AppError>
pub async fn ensure_tag(db: &Db, name: &str) -> Result<Tag, AppError>
pub async fn delete_tag(db: &Db, id: i64) -> Result<(), AppError>
```

- [ ] **Step 1: 写服务层失败测试**

`tests/services_posts.rs`（核心断言）：

```rust
mod common;
use common::test_config;
use hancic::db;
use hancic::services::posts::{self, NewPost, PostListOptions, UpdatePost};
use hancic::models::PostStatus;

async fn setup(tag: &str) -> (sqlx::SqlitePool, hancic::config::Config) {
    let cfg = test_config(tag);
    let pool = db::init(&cfg.data_dir).await.unwrap();
    (pool, cfg)
}

#[tokio::test]
async fn create_and_get_post() {
    let (pool, _cfg) = setup("create-post").await;
    let p = posts::create_post(&pool, NewPost {
        title: "Hello 世界".into(), content_md: "# 标题\n正文".into(),
        excerpt: None, slug: None, status: PostStatus::Published,
        post_type: hancic::models::PostType::Post, category_id: None,
        tags: vec!["rust".into(), "博客".into()],
    }).await.unwrap();
    assert_eq!(p.slug, "hello-世界");
    assert_eq!(p.status, PostStatus::Published);
    let fetched = posts::get_post_by_slug(&pool, "hello-世界").await.unwrap().unwrap();
    assert_eq!(fetched.title, "Hello 世界");
    let tags = posts::list_tags_of_post(&pool, p.id).await.unwrap();
    assert_eq!(tags.len(), 2);
}

#[tokio::test]
async fn slug_conflict_appends_suffix() {
    let (pool, _cfg) = setup("slug-conflict").await;
    for _ in 1..=2 {
        posts::create_post(&pool, NewPost {
            title: "同一标题".into(), content_md: "x".into(), excerpt: None, slug: None,
            status: PostStatus::Draft, post_type: hancic::models::PostType::Post,
            category_id: None, tags: vec![],
        }).await.unwrap();
    }
    let a = posts::get_post_by_slug(&pool, "同一标题").await.unwrap().unwrap();
    let b = posts::get_post_by_slug(&pool, "同一标题-2").await.unwrap().unwrap();
    assert_ne!(a.id, b.id);
}

#[tokio::test]
async fn list_published_only_and_paginate() {
    // 建 3 篇（2 发布 1 草稿），list_posts(status=Published, page=1, page_size=2)
    // 断言 total=2、len=2
}

#[tokio::test]
async fn update_and_delete() {
    // update_post 改标题与 status；delete_post 后再 get 为 None
}

#[tokio::test]
async fn adjacent_posts_ordered_by_published_at() {
    // 3 篇已发布文章，中间一篇的 adjacent：prev=前一篇、next=后一篇
}
```

`tests/services_taxonomy.rs`：

```rust
mod common;
use common::test_config;
use hancic::db;
use hancic::services::taxonomy;

#[tokio::test]
async fn category_crud_and_unique_slug() {
    let cfg = test_config("taxonomy");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    let c = taxonomy::create_category(&pool, "技术", "tech", 1).await.unwrap();
    assert_eq!(c.name, "技术");
    let dup = taxonomy::create_category(&pool, "技术二", "tech", 2).await;
    assert!(dup.is_err());
    let updated = taxonomy::update_category(&pool, c.id, "编程", "code", 0).await.unwrap();
    assert_eq!(updated.slug, "code");
    taxonomy::delete_category(&pool, c.id).await.unwrap();
    assert!(taxonomy::get_category_by_slug(&pool, "code").await.unwrap().is_none());
}

#[tokio::test]
async fn ensure_tag_dedup() {
    // ensure_tag("Rust") 两次，第二次返回同 id
    // ensure_tag("Rust") 与 ensure_tag("rust") slug 相同 → 同 id（slug 统一小写）
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --test services_posts --test services_taxonomy`
Expected: 编译失败（`hancic::services` 不存在）

- [ ] **Step 3: 实现 models**

`src/models.rs`：结构体字段与 §4 一致；**sqlx 自定义枚举以 String 存储**——models 中 `status: String`、`post_type: String`、`kind: String`，服务层提供 `PostStatus`/`PostType`/`AttachmentKind` 枚举与其互转（`to_str`/`from_str`），避免手写 sqlx 编解码。此决策影响所有下游，统一遵守。`DateTime<Utc>` 与 TEXT 存取：sqlx 的 `time` feature 自动处理 RFC3339。

- [ ] **Step 4: 实现 slugify 与文章服务**

`src/services/posts.rs`（关键实现）：

```rust
pub async fn slugify(input: &str) -> String {
    let s = input.trim().to_lowercase();
    // 保留 CJK 与字母数字，其余空白/标点转 '-'；连续分隔符合并、去首尾 '-'
    let mut out = String::new();
    let mut pending_dash = false;
    for ch in s.chars() {
        if ch.is_alphanumeric() {
            out.push(ch);
            pending_dash = false;
        } else if !out.is_empty() && !pending_dash {
            out.push('-');
            pending_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

pub async fn create_post(db: &Db, input: NewPost) -> Result<Post, AppError> {
    let slug_base = input.slug.clone().filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| input.title.clone());
    let slug = unique_slug(db, &slugify(&slug_base)).await?;
    let excerpt = input.excerpt.clone().filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| excerpt_of(&input.content_md));
    let status = input.status.to_str();
    let post_type = input.post_type.to_str();
    let published_at = if input.status == PostStatus::Published { Some(Utc::now()) } else { None };
    let id = sqlx::query(
        "INSERT INTO posts(slug,title,content_md,excerpt,status,post_type,published_at,category_id)
         VALUES (?,?,?,?,?,?,?,?)")
        .bind(&slug).bind(&input.title).bind(&input.content_md).bind(&excerpt)
        .bind(status).bind(post_type)
        .bind(published_at.map(|d| d.to_rfc3339()))
        .bind(input.category_id)
        .execute(db).await?.last_insert_rowid();
    if !input.tags.is_empty() {
        set_post_tags(db, id, &input.tags).await?;
    }
    get_post(db, id).await?.ok_or_else(|| AppError::Internal("建文后读取失败".into()))
}

async fn unique_slug(db: &Db, base: &str) -> Result<String, AppError> {
    if get_post_by_slug(db, base).await?.is_none() { return Ok(base.to_string()); }
    for i in 2..1000 {
        let candidate = format!("{base}-{i}");
        if get_post_by_slug(db, &candidate).await?.is_none() { return Ok(candidate); }
    }
    Err(AppError::Internal("slug 冲突过多".into()))
}
```

`list_posts` 动态 SQL：按 `opts` 拼接 WHERE（status/category_slug/tag_slug，均参数化），`COUNT(*)` 与 `LIMIT ? OFFSET ?` 两查询，按 `published_at DESC, id DESC` 排序。`update_post` 用 `UPDATE posts SET ... WHERE id=?`（动态拼接 set 子句，值全参数化），`updated_at=now`；若更新为 published 且原为 draft 则设 `published_at`。`adjacent_posts`：`published_at < p.published_at ORDER BY published_at DESC LIMIT 1` 与 `> p.published_at ORDER BY published_at ASC LIMIT 1`。`set_post_tags`：DELETE 全部 → 逐个 `ensure_tag` + INSERT OR IGNORE。`excerpt_of`：去掉行首 `#`、链接/图片语法后截 150 字符。

- [ ] **Step 5: 实现 taxonomy 服务**

`src/services/taxonomy.rs`（关键点）：`create_category` 检查 slug 唯一（冲突返回 `AppError::Conflict("分类 slug 已存在")`）；`ensure_tag`：按 name→slugify 查，无则插入；slug 全小写。其余为标准 CRUD。

- [ ] **Step 6: 运行测试确认通过**

Run: `cargo test --test services_posts --test services_taxonomy`
Expected: PASS（含 slug 冲突后缀、分页 total、相邻文章、标签去重）

- [ ] **Step 7: 提交**

```bash
git add -A && git commit -m "feat: 核心模型与文章/分类/标签服务层"
```

## 阶段 B：身份与主题（T4–T6）

### Task 4: 设置服务、管理员密码与 argon2

**Files:**
- Create: `src/services/settings.rs`, `src/auth.rs`
- Test: `tests/auth_password.rs`

**Interfaces:**
- Consumes: `Db`（T2）
- Produces:
  - `services::settings::get(db, key) -> Result<Option<String>>`
  - `services::settings::set(db, key, value) -> Result<()>`
  - `services::settings::get_many(db, keys: &[&str]) -> Result<HashMap<String,String>>`
  - `services::settings::all(db) -> Result<HashMap<String,String>>`
  - `auth::hash_password(pw: &str) -> Result<String>`（argon2id）
  - `auth::verify_password(pw: &str, hash: &str) -> bool`
  - `auth::has_password(db) -> Result<bool>`（`settings['admin_password_hash']` 是否存在）
  - `auth::set_password(db, pw: &str) -> Result<()>`（校验强度：≥8 字符）
  - `auth::PASSWORD_MIN_LEN: usize = 8`

- [ ] **Step 1: 写失败测试**

`tests/auth_password.rs`：

```rust
mod common;
use common::test_config;
use hancic::auth;
use hancic::db;

#[tokio::test]
async fn password_hash_verify_roundtrip() {
    let hash = auth::hash_password("correct-horse-123").unwrap();
    assert!(auth::verify_password("correct-horse-123", &hash));
    assert!(!auth::verify_password("wrong", &hash));
    let hash2 = auth::hash_password("correct-horse-123").unwrap();
    assert_ne!(hash, hash2); // 随机盐
}

#[tokio::test]
async fn set_password_requires_min_length() {
    let cfg = test_config("pw-min");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    let r = auth::set_password(&pool, "short").await;
    assert!(r.is_err());
    assert_eq!(auth::has_password(&pool).await.unwrap(), false);
    auth::set_password(&pool, "a-strong-password!").await.unwrap();
    assert!(auth::has_password(&pool).await.unwrap());
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --test auth_password`
Expected: 编译失败（`hancic::auth` 不存在）

- [ ] **Step 3: 实现 settings 与 auth**

`src/services/settings.rs`：`get/set/all/get_many` 均为单条 SQL（`SELECT value FROM settings WHERE key=?` / `INSERT INTO settings(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value`）。`get` 返回 `Ok(None)` 当键不存在。

`src/auth.rs`：

```rust
use argon2::{Argon2, PasswordHasher, PasswordVerifier, password_hash::{SaltString, rand_core::OsRng, PasswordHash}};
use crate::db::Db;
use crate::error::AppError;
use crate::services::settings;

pub const PASSWORD_MIN_LEN: usize = 8;
const SETTINGS_KEY: &str = "admin_password_hash";

pub fn hash_password(pw: &str) -> Result<String, AppError> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    argon2.hash_password(pw.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AppError::Internal(format!("密码哈希失败: {e}")))
}

pub fn verify_password(pw: &str, hash: &str) -> bool {
    match PasswordHash::new(hash).and_then(|h| {
        Argon2::default().verify_password(pw.as_bytes(), &h)
    }) {
        Ok(()) => true,
        Err(_) => false,
    }
}

pub async fn has_password(db: &Db) -> Result<bool, AppError> {
    Ok(settings::get(db, SETTINGS_KEY).await?.is_some())
}

pub async fn set_password(db: &Db, pw: &str) -> Result<(), AppError> {
    if pw.chars().count() < PASSWORD_MIN_LEN {
        return Err(AppError::BadRequest(format!("密码至少 {PASSWORD_MIN_LEN} 个字符")));
    }
    let hash = hash_password(pw)?;
    settings::set(db, SETTINGS_KEY, &hash).await
}
```

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --test auth_password`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat: 设置服务与 argon2 密码管理"
```

---

### Task 5: 登录会话、CSRF 与后台鉴权中间件

**Files:**
- Create: `src/session.rs`, `src/admin/mod.rs`（路由树 + 登录页与 setup 页 handler）
- Modify: `src/lib.rs`（挂 `/admin` 路由）
- Test: `tests/session_csrf.rs`

**Interfaces:**
- Consumes: `auth`（T4）、`settings`（T4）、`Db`
- Produces:
  - `session::session_layer(pool: &Db) -> SessionManagerLayer<SqliteStore>`（tower-sessions `SqliteStore`，表 `hancic_sessions` 自建于 hancic.db；cookie 名 `hancic_session`，httpOnly + SameSite=Lax，Secure 由部署侧 TLS 处理）
  - `session::require_admin(session: &Session) -> Result<(), AppError>`：会话无 `user_id=1` → `Unauthorized`
  - `session::login(db, session, password) -> Result<(), AppError>`：校验密码 + `session.insert("user_id", 1)`
  - `session::logout(session) -> Result<()>`
  - `session::csrf_token(session) -> Result<String>`（惰性生成 32 字节随机 hex 存 session）
  - `session::verify_csrf(session, provided: Option<&str>) -> Result<(), AppError>`
  - `session::LoginLimiter`（`Arc<Mutex<HashMap<String,(i64,i64)>>>`，同 IP 10 分钟内失败 ≥5 次拒绝）：`check(ip, allowed) -> bool` / `record_failure(ip) -> u32`
  - `AppState` 扩展：`pub db: Db`、`pub login_limiter: Arc<LoginLimiter>`

路由：
```
GET  /admin/login      → 登录页（无密码时提示去 setup）
POST /admin/login      → 校验密码 + CSRF + 限流
GET  /admin/logout     → 登出
GET  /admin/setup      → 首启设置密码页（has_password=false 才可访问）
POST /admin/setup      → 设置密码 + 自动登录
GET  /admin            → 仪表盘（require_admin；T12 完善）
```

- [ ] **Step 1: 写失败测试**

`tests/session_csrf.rs`：

```rust
mod common;
use common::test_app;
use tower::ServiceExt;
use axum::http::{Request, StatusCode};
use axum::body::Body;

#[tokio::test]
async fn setup_then_login_flow() {
    let (app, pool) = test_app("session").await;
    let res = app.clone().oneshot(Request::builder().uri("/admin/login").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    // 后续多步登录/CSRF 走 common::start_server + reqwest（见下方约定）
}
```

**测试约定修正**：`tests/common/mod.rs` 追加 `start_server(tag) -> (SocketAddr, reqwest::Client, Db)`（起随机端口 + cookie_store client）与 `login_admin(&client, &addr) -> bool`（GET /admin/login 取 `<meta name="csrf-token">` → POST /admin/login 带 password+csrf → 断言 302）。所有带登录/多步表单的集成测试统一用它。

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --test session_csrf`
Expected: 编译失败（`hancic::session`、`/admin` 路由不存在）

- [ ] **Step 3: 实现 session 层与登录/登出/setup 页面**

`src/session.rs` 要点：

```rust
use tower_sessions::{Session, SessionManagerLayer, sqlx_store::SqliteStore, Expiry};

pub fn session_layer(pool: &Db) -> SessionManagerLayer<SqliteStore> {
    // SqliteStore::new(pool.clone()).with_table_name("hancic_sessions")
    // + store.migrate()（以 tower-sessions 实际 API 为准，按编译错误修正）
    SessionManagerLayer::new(store)
        .with_secure(false)
        .with_same_site(tower_sessions::cookie::SameSite::Lax)
        .with_expiry(Expiry::OnSessionEnd)
}

pub fn require_admin(session: &Session) -> Result<(), AppError> {
    if session.get::<i64>("user_id").unwrap_or(None) == Some(1) { Ok(()) }
    else { Err(AppError::Unauthorized("请先登录".into())) }
}

pub fn csrf_token(session: &Session) -> Result<String, AppError> {
    if let Some(t) = session.get::<String>("csrf").unwrap_or(None) { return Ok(t); }
    let t = random_hex(32);
    session.insert("csrf", &t).map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(t)
}

pub fn verify_csrf(session: &Session, provided: Option<&str>) -> Result<(), AppError> {
    let expected = csrf_token(session)?;
    if provided == Some(&expected) { Ok(()) }
    else { Err(AppError::Forbidden("CSRF 校验失败".into())) }
}
```

`src/admin/mod.rs`：登录/登出/setup handler。本任务先以内联 HTML（`include_str!` + 最小模板字符串）渲染登录/setup 页，T12 替换为统一布局。登录 POST 顺序：限流检查 → CSRF → `verify_password` → `session.insert("user_id",1)` → 302 `/admin`；失败记限流 + 302 `/admin/login?error=1`。`src/lib.rs`：`pub mod session; pub mod admin; pub mod auth; pub mod services;`；`AppState { config, db, login_limiter }`；`app()` 挂 `session_layer` + `/admin` 子路由。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --test session_csrf`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat: 登录会话、CSRF 与后台鉴权中间件"
```

---

### Task 6: 主题系统（发现/加载/tera 构建）与默认主题骨架

**Files:**
- Create: `src/themes.rs`, `src/markdown.rs`（markdown filter 依赖，本任务建最小 `render`，T7 完善）, `themes/default/theme.toml`, `themes/default/templates/partials/*.html`（骨架）, `docs/theme-guide.md`
- Test: `tests/themes_load.rs`

**Interfaces:**
- Consumes: `Db`（settings 里 `active_theme`）
- Produces:
  - `themes::ThemeMeta { name, author, version, description }`（`Deserialize` 自 theme.toml，缺省空串）
  - `themes::discover(themes_dir: &Path) -> Result<Vec<ThemeMeta>>`（读每个子目录 theme.toml；损坏目录跳过并 warn）
  - `themes::load_meta(themes_dir: &Path, name: &str) -> Result<ThemeMeta>`
  - `themes::build_tera(themes_dir: &Path, name: &str) -> Result<Tera>`：注册自定义 filter（`markdown`、`date`），模板根为该主题 `templates/`（含 partials）
  - `themes::static_dir(themes_dir: &Path, name: &str) -> PathBuf`
  - `markdown::render(md: &str) -> String`：本任务最小实现（pulldown-cmark 默认选项），T7 完善选项与标题 id
  - tera filters：`markdown`（调 `markdown::render`）、`date`（按 settings timezone 格式化 `DateTime<Utc>`）
  - `AppState` 扩展：`pub tera: Tera`、`pub theme_dir: PathBuf`
  - 默认主题目录骨架：`theme.toml`、`templates/{index,post,moments,page,category,search,error}.html`（空骨架）、`templates/partials/{header,footer,pagination}.html`（空骨架）、`static/style.css`（空）

- [ ] **Step 1: 写失败测试**

`tests/themes_load.rs`：

```rust
mod common;
use common::test_config;
use hancic::db;
use hancic::themes;

#[tokio::test]
async fn discover_and_build_default_theme() {
    let cfg = test_config("themes");
    let themes_dir = cfg.data_dir.join("themes");
    std::fs::create_dir_all(&themes_dir).unwrap();
    copy_recursive("themes/default", &themes_dir.join("default"));
    let metas = themes::discover(&themes_dir).unwrap();
    assert!(metas.iter().any(|m| m.name == "default"));
    let tera = themes::build_tera(&themes_dir, "default").unwrap();
    assert!(tera.get_template("index.html").is_some());
    assert!(tera.get_template("partials/header.html").is_some());
}

#[tokio::test]
async fn missing_theme_errors() {
    let cfg = test_config("themes-missing");
    let r = themes::build_tera(&cfg.data_dir.join("themes"), "nonexistent");
    assert!(r.is_err());
}
```

（`copy_recursive` 为 `common` 辅助函数：递归复制目录。）

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --test themes_load`
Expected: 编译失败（`hancic::themes` 不存在）

- [ ] **Step 3: 实现 themes 模块与默认主题骨架**

`src/themes.rs`（要点）：

```rust
use std::path::{Path, PathBuf};
use tera::{Tera, Result as TeraResult};

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ThemeMeta {
    pub name: String,
    #[serde(default)] pub author: String,
    #[serde(default)] pub version: String,
    #[serde(default)] pub description: String,
}

pub fn discover(themes_dir: &Path) -> Result<Vec<ThemeMeta>, String> {
    let mut out = vec![];
    for e in std::fs::read_dir(themes_dir).map_err(|e| e.to_string())?.flatten() {
        if !e.path().is_dir() { continue; }
        match load_meta(themes_dir, &e.file_name().to_string_lossy()) {
            Ok(m) => out.push(m),
            Err(err) => tracing::warn!("跳过无效主题 {}: {err}", e.file_name().to_string_lossy()),
        }
    }
    Ok(out)
}

pub fn load_meta(themes_dir: &Path, name: &str) -> Result<ThemeMeta, String> {
    let f = themes_dir.join(name).join("theme.toml");
    let content = std::fs::read_to_string(&f).map_err(|e| format!("{f:?}: {e}"))?;
    let mut meta: ThemeMeta = toml::from_str(&content).map_err(|e| e.to_string())?;
    meta.name = name.to_string(); // 以目录名为准
    Ok(meta)
}

pub fn build_tera(themes_dir: &Path, name: &str) -> Result<Tera, String> {
    let tpl_dir = themes_dir.join(name).join("templates");
    let mut tera = Tera::new(&format!("{}/**/*.html", tpl_dir.display()))
        .map_err(|e| e.to_string())?;
    tera.register_filter("markdown", markdown_filter);
    tera.register_filter("date", date_filter);
    Ok(tera)
}

fn markdown_filter(value: &tera::Value, _args: &HashMap<String, tera::Value>) -> TeraResult<tera::Value> {
    let s = value.as_str().unwrap_or_default();
    Ok(tera::Value::String(crate::markdown::render(s)))
}
```

`theme.toml`：

```toml
name = "default"
author = "hancic"
version = "0.1.0"
description = "Typora 式极简默认主题"
```

`docs/theme-guide.md`：theme.toml 字段、模板契约（`site`、`post`、`posts`、`moments`、`categories`、`page`、`pagination`、`search_query`）、filter 用法、静态资源路径 `/theme/<name>/static/`、贡献方式（PR 目录，不碰 Rust）。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --test themes_load`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat: 主题系统与默认主题骨架"
```

---

## 阶段 C：前台（T7–T11）

### Task 7: 前台路由、Markdown 渲染与页面

**Files:**
- Create: `src/web/mod.rs`, `src/web/front.rs`, `themes/default/templates/index.html`, `themes/default/templates/post.html`, `themes/default/templates/page.html`, `themes/default/templates/category.html`, `themes/default/templates/error.html`, `themes/default/templates/partials/header.html`, `themes/default/templates/partials/footer.html`, `themes/default/templates/partials/pagination.html`, `themes/default/static/style.css`, `themes/default/static/main.js`
- Modify: `src/markdown.rs`（完善渲染选项）
- Test: `tests/front_pages.rs`

**Interfaces:**
- Consumes: T3 服务层、T6 `build_tera`/`static_dir`、T5 session
- Produces:
  - `markdown::render(md: &str) -> String`：pulldown-cmark（Options: ENABLE_TABLES|ENABLE_STRIKETHROUGH|ENABLE_HEADING_ATTRIBUTES|ENABLE_FOOTNOTES|ENABLE_TASKLISTS），输出 `<div class="md-body">…</div>`
  - 前台路由（挂在 `/`）：
    ```
    GET /                    首页（文章流分页 + 导航）
    GET /post/{slug}         文章页（正文/时间/分类/标签/阅读量/上一篇下一篇；T8 接 page_views）
    GET /page/{slug}         独立页（type=page）
    GET /category/{slug}     分类归档
    GET /tag/{slug}          标签归档
    GET /about               快捷路由 → /page/about（不存在则 404）
    GET /search?q=           搜索页（T9 完善，本任务渲染表单与空结果）
    GET /uploads/*path       ServeDir（附件，缓存头 public,max-age=604800）
    GET /theme/{name}/static/*path  ServeDir（主题静态资源，缓存头）
    ```
  - `web::front::site_context(db) -> Result<Context>`：`site` 对象含 `name/desc/nav/social/active_theme/mode`（settings；nav 解析 JSON 数组 `{label,url}`）
  - 亮暗色：`html[data-mode="auto|light|dark"]`，`main.js` 手动切换存 localStorage；CSS 变量双套；默认跟随 `prefers-color-scheme`
  - 移动端：`<meta viewport>`、导航 ≤768px 折叠汉堡（main.js）、正文 `max-width:720px`、图片懒加载 `loading="lazy"` + lightbox（main.js）
  - 错误页：404/500 渲染 `error.html`

- [ ] **Step 1: 写失败测试**

`tests/front_pages.rs`：

```rust
mod common;
use common::test_app;
use hancic::db;
use hancic::services::posts::{self, NewPost};
use hancic::models::PostStatus;
use tower::ServiceExt;
use axum::http::{Request, StatusCode};
use axum::body::Body;

#[tokio::test]
async fn homepage_lists_published_posts() {
    let (app, pool) = test_app("front-home").await;
    posts::create_post(&pool, NewPost {
        title: "第一篇文章".into(), content_md: "# 标题\n\n正文内容 **加粗**".into(),
        excerpt: None, slug: None, status: PostStatus::Published,
        post_type: hancic::models::PostType::Post, category_id: None,
        tags: vec!["rust".into()],
    }).await.unwrap();
    let res = app.oneshot(Request::builder().uri("/").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let html = String::from_utf8(axum::body::to_bytes(res.into_body(), 1024*1024).await.unwrap().to_vec()).unwrap();
    assert!(html.contains("第一篇文章"));
    assert!(html.contains("寒蝉 Hancic"));
}

#[tokio::test]
async fn post_page_renders_markdown() {
    // GET /post/第一篇文章 → 200，html 含 "<h1"、"正文内容"
}

#[tokio::test]
async fn unknown_slug_404() {
    // GET /post/不存在 → 404
}

#[tokio::test]
async fn category_page_filters() {
    // 建分类 tech + 1 篇发布文章归入 → GET /category/tech → 200 含标题
}

#[tokio::test]
async fn about_page_renders_page_type() {
    // 建 post_type=Page、slug=about → GET /about → 200 含标题
}
```

**测试约定**：`test_app(tag) -> (Router, SqlitePool)`（T2 已修正 async，本任务改为返回二元组，`start_server` 返回 `(addr, client, pool)`）。

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --test front_pages`
Expected: 编译失败（`hancic::web` 不存在）

- [ ] **Step 3: 实现 markdown 渲染与前台路由**

`src/markdown.rs`：

```rust
use pulldown_cmark::{html, Options, Parser};

pub fn render(md: &str) -> String {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_HEADING_ATTRIBUTES | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(md, opts);
    let mut html_out = String::new();
    html::push_html(&mut html_out, parser);
    format!("<div class=\"md-body\">{html_out}</div>")
}
```

`src/web/front.rs` 要点：`index` handler 调 `list_posts(Published, page, page_size=10)` → `site_context` → 渲染 `index.html`。`post` 路由：`get_post_by_slug` → 未发布且非草稿预览 → 404；`adjacent_posts`、`list_tags_of_post`、`get_category_by_slug` 补充 context → `post.html`。`page` 路由过滤 `post_type=page`。分类/标签归档：`list_posts` 带 filter。`uploads`/`theme static` 用 `ServeDir` + 缓存头。

`site_context` 从 settings 读 `site_name/site_desc/site_nav/site_social/active_theme/theme_mode` 构造 `site` JSON 对象。

- [ ] **Step 4: 写默认主题模板与 CSS（Typora 式）**

`partials/header.html`：

```html
<!DOCTYPE html>
<html lang="zh-CN" data-mode="{{ site.mode }}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{% block title %}{{ site.name }}{% endblock title %}</title>
<meta name="description" content="{{ site.desc }}">
<link rel="stylesheet" href="/theme/{{ site.active_theme }}/static/style.css">
</head>
<body>
<header class="site-header">
  <a class="site-brand" href="/">{{ site.name }}</a>
  <nav class="site-nav" id="site-nav">
    {% for item in site.nav %}<a href="{{ item.url }}">{{ item.label }}</a>{% endfor %}
  </nav>
  <button class="nav-toggle" id="nav-toggle" aria-label="菜单">☰</button>
</header>
<main class="site-main">
```

`footer.html`：

```html
</main>
<footer class="site-footer">
  <p>© {{ site.name }}</p>
</footer>
<script src="/theme/{{ site.active_theme }}/static/main.js"></script>
</body>
</html>
```

`index.html`：`{% include "partials/header.html" %}` 开头 + `{% include "partials/footer.html" %}` 结尾（tera include 方式），文章循环 `posts`（标题/日期/excerpt/阅读量），`pagination.html` 上一页/下一页。`post.html`：标题 + 元信息（日期/分类/标签/阅读量）+ `{{ post.content_md | markdown | safe }}` + 上一篇/下一篇。`error.html`：状态码 + 中文文案。

`style.css` 关键规则（Typora 式）：

```css
:root { --bg:#ffffff; --fg:#2c3e50; --muted:#94a3b8; --accent:#2563eb; --border:#e5e7eb;
        --content-width:720px; }
html[data-mode="dark"] { --bg:#1e1e1e; --fg:#d4d4d4; --muted:#8a8a8a; --accent:#4d9fff; --border:#333; }
@media (prefers-color-scheme: dark) { html[data-mode="auto"] { /* 同上暗色变量 */ } }
body { background: var(--bg); color: var(--fg); font: 17px/1.8 -apple-system,"PingFang SC","Noto Sans SC",sans-serif;
       margin:0; transition: background .3s,color .3s; }
.site-main { max-width: var(--content-width); margin: 0 auto; padding: 2.5rem 1.25rem 4rem; }
.site-header { max-width: var(--content-width); margin: 0 auto; padding: 1.5rem 1.25rem;
       display:flex; align-items:center; gap:1.5rem; }
.site-brand { font-weight:600; font-size:1.15rem; color:var(--fg); text-decoration:none; }
.site-nav { display:flex; gap:1.25rem; }
.site-nav a { color:var(--muted); text-decoration:none; }
.site-nav a:hover { color:var(--accent); }
.nav-toggle { display:none; background:none; border:0; font-size:1.3rem; cursor:pointer; color:var(--fg); }
.post-list-item { padding:1.75rem 0; border-bottom:1px solid var(--border); }
.post-list-item h2 a { color:var(--fg); text-decoration:none; }
.post-list-item h2 a:hover { color:var(--accent); }
.meta { color:var(--muted); font-size:.875rem; display:flex; gap:1rem; flex-wrap:wrap; }
.md-body h1,.md-body h2,.md-body h3 { line-height:1.3; margin:1.8em 0 .8em; }
.md-body img { max-width:100%; height:auto; border-radius:6px; cursor:zoom-in; }
.md-body pre { background:var(--border); padding:1rem; border-radius:8px; overflow-x:auto; }
.md-body code { font-family: ui-monospace,SFMono-Regular,Menlo,monospace; font-size:.9em; }
.md-body a { color:var(--accent); }
@media (max-width:768px) {
  .site-nav { display:none; position:absolute; top:3.2rem; left:0; right:0; background:var(--bg);
       flex-direction:column; padding:1rem 1.25rem; border-bottom:1px solid var(--border); }
  .site-nav.open { display:flex; }
  .nav-toggle { display:block; }
  .site-main { padding:1.25rem 1rem 3rem; font-size:16.5px; }
}
```

`main.js`：汉堡菜单切换、亮暗色循环切换（auto→light→dark 存 localStorage）、lightbox、图片懒加载。

- [ ] **Step 5: 运行确认通过**

Run: `cargo test --test front_pages`
Expected: PASS（5 个用例）

- [ ] **Step 6: 提交**

```bash
git add -A && git commit -m "feat: 前台路由、Markdown 渲染与 Typora 式默认主题"
```

---

### Task 8: 阅读统计（page_views 写入 + ip2region 地区解析）

**Files:**
- Create: `src/ipregion.rs`, `src/services/stats.rs`, `scripts/fetch-assets.sh`（本任务含下载 ip2region.xdb 部分）
- Modify: `src/web/front.rs`（文章页写 page_views）
- Test: `tests/stats_region.rs`

**Interfaces:**
- Consumes: `Db`、前台 post 路由、T7
- Produces:
  - `ipregion::Region { country, province, city }`
  - `ipregion::Searcher`：`new(xdb_path: &Path) -> Result<Searcher>`；`lookup(ip: &IpAddr) -> Region`（私有 IP/失败 → country="本地"）
  - `ipregion::ensure_xdb(data_dir: &Path) -> Result<PathBuf>`：若 `data_dir/ip2region.xdb` 不存在，从 `include_bytes!("../assets/ip2region.xdb")` 写出
  - `services::stats::record_view(db, post_id, ip: &str, ua: &str, referer: &str, searcher: &Searcher) -> Result<()>`：事务内 INSERT page_views（含地区）+ `UPDATE posts SET views=views+1`
  - `services::stats::summary(db, from: Option<&str>, to: Option<&str>) -> Result<StatsSummary>`；`StatsSummary { total_views, total_posts, total_moments, total_attachments, trend: Vec<DailyCount{date,count}> }`
  - `services::stats::top_posts(db, from, to, limit) -> Result<Vec<(Post, i64)>>`
  - `services::stats::by_region(db, from, to) -> Result<Vec<RegionStat{country,province,city,count}>>`
  - `services::stats::clear_logs(db) -> Result<()>`
  - `AppState` 扩展：`pub ip_searcher: Arc<Searcher>`（app 启动时 `ensure_xdb` + `Searcher::new`）
  - `scripts/fetch-assets.sh`：下载 `assets/ip2region.xdb`（`https://github.com/lionsoul2014/ip2region/raw/master/data/ip2region.xdb`，失败回退 gitee 镜像），提交入库

- [ ] **Step 1: 写失败测试**

`tests/stats_region.rs`：

```rust
mod common;
use common::test_app;
use hancic::ipregion::Searcher;
use std::net::IpAddr;

#[tokio::test]
async fn private_ip_is_local() {
    let searcher = Searcher::new(&common::xdb_path()).unwrap();
    let r = searcher.lookup(&"127.0.0.1".parse::<IpAddr>().unwrap());
    assert_eq!(r.country, "本地");
}

#[tokio::test]
async fn public_ip_resolves_region() {
    // 114.114.114.114 → country=="中国"
}

#[tokio::test]
async fn record_view_and_query_summary() {
    let (app, pool) = test_app("stats").await;
    // seed 1 篇发布文章；record_view 3 次（不同地区 ip）→ summary.total_views==3、
    // trend 含当日 3、top_posts 该文 3、by_region 汇总正确；clear_logs 后 total_views==0
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --test stats_region`
Expected: 编译失败或 xdb 文件缺失

- [ ] **Step 3: 实现 ipregion 与 stats**

`src/ipregion.rs`（用 `ip2region` crate）：

```rust
use ip2region::Searcher as RawSearcher;
use std::net::IpAddr;

pub struct Searcher { raw: RawSearcher, }

#[derive(Debug, Clone, Default)]
pub struct Region { pub country: String, pub province: String, pub city: String, }

impl Region {
    pub fn local() -> Self { Self { country: "本地".into(), province: "".into(), city: "".into() } }
}

impl Searcher {
    pub fn new(xdb_path: &std::path::Path) -> Result<Self, String> {
        Ok(Self { raw: RawSearcher::new(xdb_path.to_str().ok_or("路径无效")?)
            .map_err(|e| e.to_string())? })
    }
    pub fn lookup(&self, ip: &IpAddr) -> Region {
        if ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified() {
            return Region::local();
        }
        match self.raw.search(&ip.to_string()) {
            Ok(line) => {
                let parts: Vec<&str> = line.split('|').collect();
                Region {
                    country: parts.first().copied().unwrap_or("").to_string(),
                    province: parts.get(1).copied().unwrap_or("").to_string(),
                    city: parts.get(2).copied().unwrap_or("").to_string(),
                }
            }
            Err(_) => Region::default(),
        }
    }
}
```

（`ip2region` crate 接口以实际为准；不可用则换替代 crate，隔离在本模块。`assets/ip2region.xdb` 需先下载：`bash scripts/fetch-assets.sh`。）

`services/stats.rs`：参数化 SQL 聚合。`record_view` 两写操作同一事务。`by_region` `GROUP BY country,province,city`。`from/to` 为 `YYYY-MM-DD` 字符串，转换为当日 UTC 边界。

`web/front.rs` post 路由末尾：

```rust
let ip = headers.get("x-real-ip").and_then(|v| v.to_str().ok()).map(str::to_string)
    .or_else(|| /* socket 地址 */).unwrap_or_default();
let ua = headers.get("user-agent").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
let referer = headers.get("referer").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
let _ = stats::record_view(&state.db, post.id, &ip, &ua, &referer, &state.ip_searcher).await;
```

（优先 `x-real-ip` 适配 nginx 反代；record_view 失败仅 warn 不阻断页面。）

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --test stats_region`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat: 阅读日志写入与 ip2region 地区统计服务"
```

---

### Task 9: FTS5 全文搜索（前台 /search）

**Files:**
- Modify: `src/services/posts.rs`（`search_posts`）、`src/web/front.rs`（/search 路由）
- Create: `themes/default/templates/search.html`
- Test: `tests/search_fts.rs`

**Interfaces:**
- Consumes: `Db`（posts_fts）、T7 前台
- Produces:
  - `posts::search_posts(db, q: &str, page: i64, page_size: i64) -> Result<(Vec<SearchHit>, i64)>`
  - `struct SearchHit { post: Post, snippet: String }`（FTS5 `snippet()` + `<mark>` 高亮）

- [ ] **Step 1: 写失败测试**

`tests/search_fts.rs`：

```rust
mod common;
use common::test_app;
use hancic::db;
use hancic::services::posts::{self, NewPost};
use hancic::models::PostStatus;
use tower::ServiceExt;
use axum::http::{Request, StatusCode};
use axum::body::Body;

#[tokio::test]
async fn search_finds_matching_posts() {
    let (app, pool) = test_app("search").await;
    for (title, body) in [("Rust 所有权", "借用检查器如何工作"),
                          ("Go 并发", "goroutine 使用"),
                          ("投资笔记", "定投策略")] {
        posts::create_post(&pool, NewPost {
            title: title.into(), content_md: body.into(), excerpt: None, slug: None,
            status: PostStatus::Published, post_type: hancic::models::PostType::Post,
            category_id: None, tags: vec![],
        }).await.unwrap();
    }
    let res = app.oneshot(Request::builder().uri("/search?q=Rust").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let html = String::from_utf8(axum::body::to_bytes(res.into_body(), 1024*1024).await.unwrap().to_vec()).unwrap();
    assert!(html.contains("Rust 所有权"));
    assert!(!html.contains("Go 并发"));
    assert!(html.contains("<mark>Rust</mark>") || html.contains("<mark>rust</mark>"));
}

#[tokio::test]
async fn search_empty_query_returns_prompt() {
    // /search?q= → 200 空结果提示
}

#[tokio::test]
async fn search_escapes_special_chars() {
    // q = `" -- 恶意'` 不崩溃且不返回全部文章
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --test search_fts`
Expected: 编译失败（`search_posts` 不存在）

- [ ] **Step 3: 实现 FTS5 搜索**

`src/services/posts.rs`：

```rust
pub struct SearchHit { pub post: Post, pub snippet: String }

pub async fn search_posts(db: &Db, q: &str, page: i64, page_size: i64)
    -> Result<(Vec<SearchHit>, i64), AppError> {
    let q = q.trim();
    if q.is_empty() { return Ok((vec![], 0)); }
    let escaped = q.replace('"', "\"\"");
    let match_expr = format!("\"{escaped}\"");
    let offset = (page - 1).max(0) * page_size;
    let rows: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT rowid, rank FROM posts_fts WHERE posts_fts MATCH ? ORDER BY rank LIMIT ? OFFSET ?")
        .bind(&match_expr).bind(page_size).bind(offset).fetch_all(db).await?;
    let mut hits = vec![];
    for (id, _) in rows {
        let post = get_post(db, id).await?.ok_or_else(|| AppError::Internal("FTS 命中丢失".into()))?;
        let snippet: String = sqlx::query_scalar(
            "SELECT snippet(posts_fts, 1, '<mark>', '</mark>', '…', 12)
             FROM posts_fts WHERE rowid = ? AND posts_fts MATCH ?")
            .bind(id).bind(&match_expr).fetch_one(db).await.unwrap_or_default();
        hits.push(SearchHit { post, snippet });
    }
    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM posts_fts WHERE posts_fts MATCH ?").bind(&match_expr)
        .fetch_one(db).await?;
    Ok((hits, total))
}
```

（`snippet(posts_fts, 1, ...)` 列索引 1=content_md；空 snippet 回退 `post.excerpt`。）

`web/front.rs` /search 路由：解析 `q` → `search_posts` → `search.html`（查询框 + 结果 + 分页 + 空态）。`search.html` 模板：表单 `GET /search` + 结果循环。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --test search_fts`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat: FTS5 全文搜索与搜索页"
```
### Task 10: 上传管线与图片压缩（POST /api/uploads）

**Files:**
- Create: `src/services/uploads.rs`, `src/api/mod.rs`, `src/api/uploads.rs`
- Modify: `src/lib.rs`（挂 `/api` 路由）
- Test: `tests/upload_compress.rs`

**Interfaces:**
- Consumes: `Db`、`Config`（上传上限与压缩参数）、`AppState`
- Produces:
  - `services::uploads::detect_kind(mime: &str, ext: &str) -> Result<AttachmentKind, AppError>`（mime + 扩展名双校验）
  - `services::uploads::save_bytes(db, cfg: &Config, uploads_dir: &Path, orig_name: &str, mime: &str, data: &[u8]) -> Result<Attachment, AppError>`（**P1 修正**：签名含 `cfg: &Config`）：白名单/大小校验 → UUID 命名 → 图片压缩 → 写盘 → INSERT attachments
  - `services::uploads::save_upload_multipart(db, cfg, uploads_dir, mut parts: Multipart) -> Result<Vec<Attachment>, AppError>`（一次多文件，单文件失败跳过并 warn，至少一个成功才 Ok）
  - `services::uploads::compress_image(path: &Path, data: &[u8], max_edge: u32, quality: u8) -> Result<Vec<u8>>`（image crate；GIF 原样返回）
  - `services::uploads::get_attachment(db, id) -> Result<Option<Attachment>>`
  - `services::uploads::delete_attachment(db, uploads_dir, id) -> Result<()>`（删文件 + 删 DB 行）
  - 白名单（mime, 扩展名）：image `image/jpeg`(jpg/jpeg)/`image/png`(png)/`image/webp`(webp)/`image/gif`(gif) ≤`upload_max_image`；video `video/mp4`(mp4)/`video/webm`(webm)/`video/quicktime`(mov) ≤`upload_max_video`；file `application/pdf`(pdf)/`text/plain`(txt)/`application/zip`(zip)/`application/x-gzip`(gz)/`application/octet-stream`(bin)/`text/markdown`(md) ≤`upload_max_file`
  - API 路由：`POST /api/uploads`（multipart 字段名 `files`；鉴权 admin session 或 Bearer，Bearer 校验 T21 补全，本任务只认 session；响应 `{"data":[Attachment...]}`）
  - 落盘路径：`<data>/uploads/{image|video|file}/<uuid>.<ext>`，DB `path` 列存相对路径 `{sub}/{uuid}.{ext}`

- [ ] **Step 1: 写失败测试**

`tests/upload_compress.rs`：

```rust
mod common;
use common::test_app;
use hancic::services::uploads;
use tower::ServiceExt;
use axum::http::{Request, StatusCode};
use axum::body::Body;

#[tokio::test]
async fn upload_small_png_creates_attachment() {
    // multipart POST /api/uploads（admin 已登录 cookie）带 PNG_1x1 → 200、kind=image、
    // DB 有记录、磁盘文件存在
}

#[tokio::test]
async fn upload_rejects_disallowed_type() {
    // 上传 application/x-msdownload → 400，消息含"不支持"
}

#[tokio::test]
async fn upload_rejects_oversize() {
    // > upload_max_image 的假数据 → 400/413
}

#[tokio::test]
async fn compress_image_downscales_large_jpeg() {
    // image 生成 4000x3000 JPEG → compress_image(max_edge=2000) → 输出 2000x1500、字节更小
}

#[tokio::test]
async fn gif_is_not_compressed() {
    // 极小 GIF → 原样返回（长度不变）
}
```

（`PNG_1x1` 常量放 `tests/common/mod.rs`。）

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --test upload_compress`
Expected: 编译失败（`hancic::services::uploads` 不存在）

- [ ] **Step 3: 实现上传服务**

`src/services/uploads.rs` 要点：

```rust
use axum::extract::Multipart;
use image::GenericImageView;
use uuid::Uuid;
use crate::config::Config;
use crate::models::AttachmentKind;

pub fn detect_kind(mime: &str, ext: &str) -> Result<AttachmentKind, AppError> {
    let ext = ext.trim_start_matches('.').to_lowercase();
    let img = [("image/jpeg", ["jpg","jpeg"]), ("image/png", ["png"]),
               ("image/webp", ["webp"]), ("image/gif", ["gif"])];
    if img.iter().any(|(m, exts)| m == &mime && exts.contains(&ext.as_str())) { return Ok(AttachmentKind::Image); }
    // video / file 同理（见 Interfaces 白名单）
    Err(AppError::BadRequest(format!("不支持的文件类型: {mime} / .{ext}")))
}

pub async fn save_bytes(db: &Db, cfg: &Config, uploads_dir: &Path, orig_name: &str, mime: &str, data: &[u8])
    -> Result<Attachment, AppError> {
    let ext = std::path::Path::new(orig_name).extension().and_then(|e| e.to_str()).unwrap_or("");
    let kind = detect_kind(mime, ext)?;
    let max = match kind {
        AttachmentKind::Image => cfg.upload_max_image,
        AttachmentKind::Video => cfg.upload_max_video,
        AttachmentKind::File => cfg.upload_max_file,
    };
    if data.len() as u64 > max {
        return Err(AppError::BadRequest(format!("文件超过大小上限（{max} 字节）")));
    }
    let uuid_name = format!("{}.{}", Uuid::new_v4(), ext);
    let sub = match kind { AttachmentKind::Image => "image", AttachmentKind::Video => "video", AttachmentKind::File => "file" };
    let dir = uploads_dir.join(sub);
    std::fs::create_dir_all(&dir).map_err(internal)?;
    let stored = if kind == AttachmentKind::Image && mime != "image/gif" && cfg.image_compress {
        compress_image(&dir.join(&uuid_name), data, cfg.image_max_edge, cfg.image_quality)?
    } else { data.to_vec() };
    std::fs::write(dir.join(&uuid_name), &stored).map_err(internal)?;
    let id = sqlx::query("INSERT INTO attachments(uuid_name,orig_name,mime,size,kind,path) VALUES (?,?,?,?,?,?)")
        .bind(&uuid_name).bind(orig_name).bind(mime)
        .bind(stored.len() as i64).bind(kind.as_str()).bind(format!("{sub}/{uuid_name}"))
        .execute(db).await?.last_insert_rowid();
    get_attachment(db, id).await?.ok_or_else(|| AppError::Internal("附件落库失败".into()))
}

pub fn compress_image(path: &Path, data: &[u8], max_edge: u32, quality: u8) -> Result<Vec<u8>, AppError> {
    let img = image::load_from_memory(data).map_err(|e| AppError::BadRequest(format!("图片解码失败: {e}")))?;
    let (w, h) = img.dimensions();
    let scaled = if w.max(h) > max_edge {
        let ratio = max_edge as f32 / w.max(h) as f32;
        img.resize((w as f32 * ratio) as u32, (h as f32 * ratio) as u32, image::imageops::FilterType::Lanczos3)
    } else { img };
    let fmt = image::ImageFormat::from_path(path).unwrap_or(image::ImageFormat::Jpeg);
    if fmt == image::ImageFormat::Gif { return Ok(data.to_vec()); }
    let mut out = std::io::Cursor::new(Vec::new());
    if fmt == image::ImageFormat::Jpeg {
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
            .encode_image(&scaled).map_err(internal)?;
    } else {
        scaled.write_to(&mut out, fmt).map_err(internal)?;
    }
    Ok(out.into_inner())
}
```

`save_upload_multipart`：循环 `parts.next_field()`，`field_name()=="files"`，`bytes().await`，逐个 `save_bytes`，错误 warn 后继续。

`src/api/mod.rs`：

```rust
pub fn router() -> Router<AppState> {
    Router::new().route("/uploads", post(crate::api::uploads::upload))
    // 后续任务挂 posts/moments/...
}
```

`src/lib.rs`：`pub mod api;`，`app()` 里 `.nest("/api", api::router())`。上传 handler 鉴权：先 `require_admin(session)`，失败再查 Bearer（T21 接上）。`internal` 为 `AppError::Internal` 简写辅助（`fn internal(e: impl Display) -> AppError`）。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --test upload_compress`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat: 附件上传管线与图片压缩"
```

---

### Task 11: 说说（模型服务 + 前台朋友圈式展示）

**Files:**
- Create: `src/services/moments.rs`, `themes/default/templates/moments.html`
- Modify: `src/web/front.rs`（/moments 路由）、`themes/default/static/style.css`（宫格样式）
- Test: `tests/moments_flow.rs`

**Interfaces:**
- Consumes: `uploads`（T10）、前台框架（T7）、settings timezone
- Produces:
  - `services::moments::create_moment(db, content: &str, attachment_ids: &[i64]) -> Result<Moment>`
  - `services::moments::list_moments(db, page, page_size) -> Result<(Vec<Moment>, i64)>`
  - `services::moments::list_moment_attachments(db, moment_id) -> Result<Vec<(Attachment, i64)>>`（按 sort_order）
  - `services::moments::delete_moment(db, moment_id) -> Result<()>`（关联级联）
  - `services::moments::group_by_day(db, moments: Vec<Moment>) -> Result<Vec<(String, Vec<Moment>)>>`（按 settings timezone 转本地取 `YYYY-MM-DD` 分组）
  - 前台路由 `GET /moments`：分页（20/页）→ group_by_day → 渲染（1 图大图 / 2-4 图 2 列 / 5+ 图 3 列宫格；视频 `<video controls preload="metadata">`；文件附件卡片）
  - `cargo add chrono-tz`（本任务新增依赖）

- [ ] **Step 1: 写失败测试**

`tests/moments_flow.rs`：

```rust
mod common;
use common::test_app;
use hancic::services::moments;
use hancic::services::uploads;
use tower::ServiceExt;
use axum::http::{Request, StatusCode};
use axum::body::Body;

#[tokio::test]
async fn create_and_group_by_day() {
    let (app, pool, data_dir) = test_app("moments").await;
    let cfg = /* test_config */;
    let a1 = uploads::save_bytes(&pool, &cfg, &data_dir.join("uploads"), "a.png", "image/png", PNG_1x1).await.unwrap();
    let a2 = uploads::save_bytes(&pool, &cfg, &data_dir.join("uploads"), "b.png", "image/png", PNG_1x1).await.unwrap();
    let m = moments::create_moment(&pool, "今天天气不错", &[a1.id, a2.id]).await.unwrap();
    let atts = moments::list_moment_attachments(&pool, m.id).await.unwrap();
    assert_eq!(atts.len(), 2);
    // 手工把第二条 created_at 改为昨天 → group_by_day 得 2 组
}

#[tokio::test]
async fn moments_page_shows_grid() {
    // GET /moments → 200，html 含"今天天气不错"与 class="moment-grid"
}

#[tokio::test]
async fn delete_moment_cascades() {
    // delete_moment 后 list_moment_attachments 为空；attachments 记录保留
}
```

（`test_app(tag) -> (Router, Db, PathBuf /*data_dir*/)` 本任务起生效。）

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --test moments_flow`
Expected: 编译失败（`services::moments` 不存在）

- [ ] **Step 3: 实现 moments 服务**

`create_moment` 事务内 INSERT moments + 循环 INSERT moment_attachments（sort_order 按数组序）。`group_by_day`：`chrono_tz::Tz::from_str(settings timezone)`（默认 Asia/Shanghai）转本地取日期前缀。`web/front.rs` /moments：context `days: Vec<{date, moments: Vec<{moment, attachments}>}>`。模板 `moments.html`：日期徽标 + 说说内容（markdown 渲染）+ 宫格。宫格 CSS：`.moment-grid { display:grid; grid-template-columns:repeat(3,1fr); gap:6px; }`，1 图 `.single` 单列大图，2-4 图 2 列，视频/文件卡片样式。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --test moments_flow`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat: 说说模型、朋友圈式按天分组与前台展示"
```

---

## 阶段 D：后台（T12–T20）

### Task 12: 后台框架（布局 + 仪表盘 + 资源准备）

**Files:**
- Create: `scripts/fetch-assets.sh`（补全 Vditor/Chart.js 下载）, `assets/vendor/`（脚本产物）, `assets/admin.css`, `assets/admin.js`, `assets/admin_templates/{layout,login,setup,dashboard}.html`
- Modify: `src/admin/mod.rs`（布局渲染 + 仪表盘）、`src/lib.rs`（`tera_admin`）、`tests/common/mod.rs`（login_admin/csrf_token）
- Test: `tests/admin_flow.rs`

**Interfaces:**
- Consumes: T5（登录/CSRF）、T4（settings）、T3（count_posts）
- Produces:
  - 后台模板集（`assets/admin_templates/`，`include_str!` 注册进 `tera_admin`）：`layout.html`（侧边栏 11 模块 + 内容区 + CSRF meta）、`login.html`、`setup.html`、`dashboard.html`
  - `scripts/fetch-assets.sh`：下载 `vditor.min.js`、`vditor.min.css`（Vditor release tar）、`chart.umd.min.js`（Chart.js）、`ip2region.xdb`（T8 已建）到 `assets/`；失败退出非零
  - 静态路由：`/static/*` → ServeDir `assets/`（含 vendor/、admin.css、admin.js）
  - `AppState` 扩展：`pub tera_admin: Tera`
  - 仪表盘：文章/说说/附件总数、总阅读、近 30 日趋势（Chart.js）、最近 5 篇草稿
  - 后台全局：admin.js（抽屉/确认弹窗 `data-confirm`/CSRF 注入）、admin.css（响应式：≤768px 侧边栏变抽屉）

- [ ] **Step 1: 写失败测试**

`tests/admin_flow.rs`：

```rust
mod common;
use common::{login_admin, start_server, test_config};
use hancic::db;
use hancic::auth;

#[tokio::test]
async fn dashboard_requires_login_and_shows_counts() {
    let cfg = test_config("admin-dash");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    auth::set_password(&pool, "test-password-123").await.unwrap();
    let (addr, client) = start_server_with_cfg(cfg).await;
    let res = client.get(format!("http://{addr}/admin")).send().await.unwrap();
    assert_eq!(res.status(), 302); // 未登录跳转
    assert!(login_admin(&client, &addr).await);
    let res = client.get(format!("http://{addr}/admin")).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let html = res.text().await.unwrap();
    assert!(html.contains("仪表盘"));
}
```

（`start_server_with_cfg(cfg)` 与 `login_admin`、`csrf_token(client, addr)` 放 `tests/common/mod.rs`。）

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --test admin_flow`
Expected: 编译失败（`tera_admin`、仪表盘路由缺失）

- [ ] **Step 3: 实现资源脚本与后台模板**

`scripts/fetch-assets.sh`（含多源回退）：

```bash
#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p assets/vendor
fetch() { # url 目标 [backup_url...]
  local url="$1" dst="$2"; shift 2
  if curl -fsSL --max-time 120 -o "$dst" "$url"; then return 0; fi
  for b in "$@"; do if curl -fsSL --max-time 120 -o "$dst" "$b"; then return 0; fi; done
  echo "下载失败: $dst" >&2; return 1
}
fetch "https://github.com/Vditor/vditor/releases/latest/download/vditor.tar.gz" /tmp/vditor.tar.gz \
      "https://mirror.ghproxy.com/https://github.com/Vditor/vditor/releases/latest/download/vditor.tar.gz"
tar -xzf /tmp/vditor.tar.gz -C /tmp/vditor-dist 2>/dev/null || true
cp /tmp/vditor-dist/vditor.min.js /tmp/vditor-dist/vditor.min.css assets/vendor/ 2>/dev/null || \
  (cd /tmp && mkdir -p vditor && tar -xzf vditor.tar.gz -C vditor && \
   cp vditor/dist/vditor.min.js vditor/dist/vditor.min.css ../assets/vendor/)
fetch "https://github.com/chartjs/Chart.js/releases/latest/download/chart.umd.min.js" assets/vendor/chart.umd.min.js \
      "https://cdn.jsdelivr.net/npm/chart.js@4/dist/chart.umd.min.js"
fetch "https://github.com/lionsoul2014/ip2region/raw/master/data/ip2region.xdb" assets/ip2region.xdb \
      "https://gitee.com/lionsoul/ip2region/raw/master/data/ip2region.xdb"
echo "assets 就绪"
```

（Vditor release 产物结构以实际为准，`tar -tzf` 检查后调整解包路径。`assets/` 全部提交入库。）

`assets/admin_templates/layout.html`（核心）：

```html
<!DOCTYPE html>
<html lang="zh-CN" data-mode="auto">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="csrf-token" content="{{ csrf }}">
<title>{% block title %}管理后台{% endblock %} · {{ site_name }}</title>
<link rel="stylesheet" href="/static/admin.css">
</head>
<body>
<div class="admin-shell">
  <aside class="admin-side" id="admin-side">
    <div class="admin-brand">{{ site_name }}<small>管理后台</small></div>
    <nav class="admin-nav">
      {% for item in admin_nav %}
      <a href="{{ item.url }}" class="{% if item.active %}active{% endif %}">{{ item.label }}</a>
      {% endfor %}
    </nav>
    <a class="admin-logout" href="/admin/logout">退出登录</a>
  </aside>
  <div class="admin-main">
    <button class="admin-toggle" id="admin-toggle">☰</button>
    <div class="admin-content">{% block content %}{% endblock %}</div>
  </div>
</div>
<script src="/static/admin.js"></script>
{% block scripts %}{% endblock %}
</body>
</html>
```

`admin_nav` 由 `admin::mod.rs` 提供（11 项 + "查看站点"），`active` 按当前路径匹配。`admin.js`：抽屉切换、`data-confirm` 拦截、POST 表单/请求自动带 CSRF（meta → header `X-CSRF-Token`）。仪表盘 handler：聚合计数 + `stats::summary` trend → `dashboard.html`（`<canvas id="trend">` + `window.chartData` + Chart.js）。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --test admin_flow`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat: 后台框架布局、仪表盘与前端资源管线"
```

---

### Task 13: 文章管理后台（Vditor 编辑器 + 列表 + 自动保存）

**Files:**
- Create: `src/admin/posts.rs`, `assets/admin_templates/posts_list.html`, `assets/admin_templates/post_edit.html`
- Modify: `src/admin/mod.rs`
- Test: `tests/admin_posts.rs`

**Interfaces:**
- Consumes: T3（posts 服务）、T5（鉴权）、T10（上传）、T12（布局）
- Produces:
  - 路由（require_admin + POST 过 CSRF）：
    ```
    GET  /admin/posts                    列表（筛选：状态/分类/关键词，分页）
    GET  /admin/posts/new                新建页
    POST /admin/posts                    创建（form：title/content_md/slug/status/category_id/tags/excerpt）
    GET  /admin/posts/{id}/edit          编辑页
    POST /admin/posts/{id}/update        保存（draft 或 publish）
    POST /admin/posts/{id}/delete        删除
    POST /admin/posts/{id}/autosave      自动保存（body=content_md）
    ```
  - Vditor 集成：`post_edit.html` 引 `/static/vendor/vditor.min.js` + css，`new Vditor("editor", { mode:"ir", cache:false, upload:{ handler: vditorUpload } })`；`vditorUpload`（admin.js）：FormData `files` → fetch `/api/uploads`（带 CSRF）→ 返回 Vditor 格式 `{"msg":"","code":0,"data":{"errFiles":[],"succMap":{...}}}`
  - 自动保存：`setInterval(30s)` + `pagehide` 时内容变化 → POST autosave；显示"已保存 HH:MM"

- [ ] **Step 1: 写失败测试**

`tests/admin_posts.rs`：

```rust
mod common;
use common::{login_admin, start_server_with_cfg, csrf_token, test_config};
use hancic::db;
use hancic::auth;
use hancic::services::posts;

#[tokio::test]
async fn create_publish_edit_delete_flow() {
    let cfg = test_config("admin-posts");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    auth::set_password(&pool, "test-password-123").await.unwrap();
    let (addr, client) = start_server_with_cfg(cfg).await;
    assert!(login_admin(&client, &addr).await);
    let csrf = csrf_token(&client, &addr).await;
    let res = client.post(format!("http://{addr}/admin/posts"))
        .form(&[("title","管理端文章"),("content_md","# 正文\n内容"),("status","draft"),
                ("category_id",""),("tags",""),("csrf",csrf.as_str())])
        .send().await.unwrap();
    assert_eq!(res.status(), 302);
    let p = posts::get_post_by_slug(&pool, "管理端文章").await.unwrap().unwrap();
    assert_eq!(p.status, hancic::models::PostStatus::Draft);
    // 发布 → Published；GET 编辑页含正文；删除后 get 为 None
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --test admin_posts`
Expected: 编译失败（`admin::posts` 不存在）

- [ ] **Step 3: 实现**

`src/admin/posts.rs`：列表组合筛选（`PostListOptions`）；表单 `Form<HashMap<String,String>>` → `NewPost`（tags 逗号分隔，excerpt 空则服务层生成）；slug 冲突错误回显编辑页。`post_edit.html`：标题输入 + 分类 select + 标签输入 + Vditor 容器 + 存草稿/发布按钮 + 隐藏 status。编辑页输出 `window._post = { id, csrf }`；admin.js 初始化 Vditor 时读 `data-content`。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --test admin_posts`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat: 后台文章管理（Vditor IR 编辑器与自动保存）"
```

---

### Task 14: 说说管理后台

**Files:**
- Create: `src/admin/moments.rs`, `assets/admin_templates/moments.html`
- Modify: `src/admin/mod.rs`
- Test: `tests/admin_moments.rs`

**Interfaces:**
- Consumes: T11（moments 服务）、T10（上传）、T13（表单模式）
- Produces:
  - 路由：
    ```
    GET  /admin/moments              列表（分页，含附件缩略）
    POST /admin/moments              发布（form：content + attachment_ids 逗号分隔）
    POST /admin/moments/{id}/delete  删除
    ```
  - 发布框：朋友圈式（大输入框 + "添加图片/视频"上传 + 已选附件缩略列表 + 发布按钮）；上传走 `/api/uploads`（session），返回 id 追加隐藏字段

- [ ] **Step 1: 写失败测试**

`tests/admin_moments.rs`：登录 → 上传两图拿 id → POST 发布说说 → list_moments 断言；GET /admin/moments 含内容与缩略 img；删除后消失。

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --test admin_moments`
Expected: 编译失败

- [ ] **Step 3: 实现**

`create` handler 解析 `content` + `attachment_ids`（split(',') 过滤空 parse），调 `moments::create_moment`。列表页取 `list_moments` + 每条 `list_moment_attachments`（缩略 `<img src="/uploads/{path}">`，视频图标）。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --test admin_moments`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat: 后台说说管理（朋友圈式发布框）"
```

---

### Task 15: 附件库后台

**Files:**
- Create: `src/admin/attachments.rs`, `assets/admin_templates/attachments.html`
- Modify: `src/admin/mod.rs`
- Test: `tests/admin_attachments.rs`

**Interfaces:**
- Consumes: T10（uploads 服务）
- Produces:
  - 路由：
    ```
    GET  /admin/attachments         列表（kind 筛选，分页，卡片网格）
    POST /admin/attachments/{id}/delete  删除
    GET  /admin/attachments/upload  上传页（拖拽/多选 → POST /api/uploads → 回列表）
    ```

- [ ] **Step 1: 写失败测试**

`tests/admin_attachments.rs`：登录 → 上传 2 文件 → 列表含两者 + kind 筛选 → 删除其一 → 列表少一、磁盘文件删除。

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --test admin_attachments`
Expected: 编译失败

- [ ] **Step 3: 实现**

列表按 `kind` 筛选（可空）+ 分页；删除调 `uploads::delete_attachment`。卡片网格（image 缩略 / video 图标+大小 / file 图标+名+大小）+ `data-confirm` 删除。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --test admin_attachments`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat: 后台附件库"
```

---

### Task 16: 分类标签管理后台

**Files:**
- Create: `src/admin/taxonomy.rs`, `assets/admin_templates/taxonomy.html`
- Modify: `src/admin/mod.rs`
- Test: `tests/admin_taxonomy.rs`

**Interfaces:**
- Consumes: T3（taxonomy 服务）
- Produces:
  - 路由：
    ```
    GET  /admin/taxonomy              分类 + 标签两栏页
    POST /admin/taxonomy/categories   新建（name/slug/sort_order）
    POST /admin/taxonomy/categories/{id}/update
    POST /admin/taxonomy/categories/{id}/delete
    POST /admin/taxonomy/tags          新建（name）
    POST /admin/taxonomy/tags/{id}/delete
    ```

- [ ] **Step 1: 写失败测试**

`tests/admin_taxonomy.rs`：建分类/标签 → 列表含；slug 冲突 → 错误提示；删除分类后文章 category_id 置空；删除标签后 post_tags 清空。

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --test admin_taxonomy`
Expected: 编译失败

- [ ] **Step 3: 实现**

两栏布局页；POST 后 302 回列表带 `?msg=` 提示。删除分类调 `taxonomy::delete_category`（ON DELETE SET NULL）。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --test admin_taxonomy`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat: 后台分类与标签管理"
```

---

### Task 17: 站点设置后台

**Files:**
- Create: `src/admin/settings.rs`, `assets/admin_templates/settings.html`
- Modify: `src/admin/mod.rs`
- Test: `tests/admin_settings.rs`

**Interfaces:**
- Consumes: T4（settings/auth）、T12（布局）
- Produces:
  - 路由：
    ```
    GET  /admin/settings
    POST /admin/settings/save     保存（site_name/site_desc/site_nav JSON/site_social JSON/theme_mode/timezone）
    POST /admin/settings/password  修改密码（old_password/new_password/confirm）
    ```
  - 校验：site_name 非空；site_nav 必须 JSON 数组（每项 label/url 非空）；theme_mode ∈ auto|light|dark；timezone 合法（chrono-tz）；新密码 ≥8 且需验证旧密码

- [ ] **Step 1: 写失败测试**

`tests/admin_settings.rs`：改 site_name → settings 表更新 + 前台首页显示新名；非法 nav JSON → 错误回显不落库；改密码：旧密码错 → 拒绝；正确 → 新密码可登录。

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --test admin_settings`
Expected: 编译失败

- [ ] **Step 3: 实现**

表单 → `settings::set` 逐个写；错误收集渲染回表单（保留已填值）。改密码：`verify_password(old, hash)` → `set_password(new)`。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --test admin_settings`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat: 后台站点设置与密码修改"
```

---

### Task 18: 主题管理后台

**Files:**
- Create: `src/admin/themes.rs`, `assets/admin_templates/themes.html`
- Modify: `src/admin/mod.rs`、`src/web/front.rs`（预览参数覆盖）
- Test: `tests/admin_themes.rs`

**Interfaces:**
- Consumes: T6（themes::discover/load_meta）、T4（settings）
- Produces:
  - 路由：
    ```
    GET  /admin/themes              已装主题列表
    POST /admin/themes/{name}/activate  切换（写 active_theme + 提示重启生效）
    GET  /admin/themes/{name}/preview  302 → /?theme_preview={name}
    ```
  - 前台 `?theme_preview=`：`site_context` 接受 `Option<String>` 覆盖 `active_theme`（只读渲染，不落库）

- [ ] **Step 1: 写失败测试**

`tests/admin_themes.rs`：GET /admin/themes 含 "default"；复制 `themes/test-theme`（最小 theme.toml + index.html）后 discover 出现；activate 后 settings `active_theme=test-theme`；preview 请求 200。

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --test admin_themes`
Expected: 编译失败

- [ ] **Step 3: 实现**

列表 + 当前标记；activate 写 settings；preview 302 到前台带 query。`front.rs` 的 `site_context(db, preview: Option<String>)`。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --test admin_themes`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat: 后台主题管理（切换与预览）"
```
### Task 19: 统计模块后台

**Files:**
- Create: `src/admin/stats.rs`, `assets/admin_templates/stats.html`
- Modify: `src/admin/mod.rs`
- Test: `tests/admin_stats.rs`

**Interfaces:**
- Consumes: T8（stats 服务）、T12（Chart.js 资源）
- Produces:
  - 路由：
    ```
    GET  /admin/stats?from=&to=    总览卡片 + 近 30 日趋势图（Chart.js 折线）
    GET  /admin/stats/posts?from=&to=   按文章排行（浏览量降序，分页）
    GET  /admin/stats/regions?from=&to=&country=&province=   地区下钻：国家→省→市
    POST /admin/stats/clear       清理全部阅读日志（二次确认）
    ```
  - 参数校验：from/to 为 `YYYY-MM-DD`，缺省近 30 天；非法 → 400 提示

- [ ] **Step 1: 写失败测试**

`tests/admin_stats.rs`：seed 文章 + 手工 INSERT page_views（2 条不同国家、1 条省市）→ GET /admin/stats 总览数正确、趋势含当日；GET /admin/stats/regions 含"中国"且下钻省正确；POST clear 后总览为 0。

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --test admin_stats`
Expected: 编译失败

- [ ] **Step 3: 实现**

`src/admin/stats.rs`：复用 `stats::summary/top_posts/by_region/clear_logs`，`from/to` 解析为 ISO 边界传入。`stats.html`：卡片区 + `<canvas id="trend">` + 地区表格（国家行可点入省）。Chart.js 数据内联 `window.chartData`。地区下钻用 query 参数回填筛选表单。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --test admin_stats`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat: 后台统计模块（趋势/排行/地区下钻）"
```

---

### Task 20: API Token 管理后台

**Files:**
- Create: `src/services/tokens.rs`, `src/admin/tokens.rs`, `assets/admin_templates/tokens.html`
- Modify: `src/admin/mod.rs`
- Test: `tests/admin_tokens.rs`

**Interfaces:**
- Consumes: T5（鉴权）
- Produces:
  - `services::tokens::generate(db, name) -> Result<(ApiToken, String /*明文*/)>`：明文 `hc_` + 32 字节 base64url（43 字符）；库存 sha256 hex
  - `services::tokens::hash(raw: &str) -> String`（sha256 hex）
  - `services::tokens::verify(db, raw: &str) -> bool`（查未吊销且 hash 匹配）
  - `services::tokens::list(db) -> Result<Vec<ApiToken>>`
  - `services::tokens::revoke(db, id) -> Result<()>`（设 revoked_at）
  - 路由：
    ```
    GET  /admin/tokens              列表（名称/前缀/创建时间/状态）
    POST /admin/tokens              生成（name）→ 302 到 ?created=1，明文显示一次（新页面）
    POST /admin/tokens/{id}/revoke  吊销
    ```

- [ ] **Step 1: 写失败测试**

`tests/admin_tokens.rs`：生成 → 明文以 `hc_` 开头且库中存的是 64 位 hex 哈希（非明文）；verify(明文)==true；吊销后 verify==false；GET /admin/tokens 列表含名称。

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --test admin_tokens`
Expected: 编译失败

- [ ] **Step 3: 实现**

`services/tokens.rs`：`generate` 用 `rand::rngs::OsRng` 填 32 字节 → `base64::engine::general_purpose::URL_SAFE_NO_PAD` 编码 → `sha2::Sha256` 哈希 hex 入库。`verify`：SELECT 未吊销行比对 `constant_time_eq`（subtle crate；或直接 `==`——**选定**：`cargo add subtle`，用 `subtle::ConstantTimeEq` 防时序）。明文展示页 `tokens_created.html`：红色警示"仅显示一次，请立即复制"。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --test admin_tokens`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat: API Token 生成、展示与吊销"
```

---

## 阶段 E：REST API（T21）

### Task 21: REST API 全套（Bearer 鉴权 + 统一 JSON）

**Files:**
- Create: `src/api/auth.rs`, `src/api/posts.rs`, `src/api/moments.rs`, `src/api/categories.rs`, `src/api/stats.rs`, `src/api/backup.rs`, `docs/ai-integration.md`
- Modify: `src/api/mod.rs`（挂全路由 + Bearer 中间件）、`src/api/uploads.rs`（支持 Bearer）、`src/lib.rs`
- Test: `tests/api_auth.rs`, `tests/api_posts.rs`

**Interfaces:**
- Consumes: T3/T8/T11/T20（服务层）、T10（上传）、T22 备份服务（本任务只挂路由，`GET /api/backup` 调 T22 的服务函数——**顺序修正**：T22 备份服务在本任务前完成，或本任务先挂占位 501——**选定**：把备份服务提前，`services::backup` 在 T22 建但 T21 的 `/api/backup` 端点也依赖它；任务顺序调整为 T21 之前完成 T22 服务层（不建后台页），T22 再补后台页面与恢复）
- Produces:
  - `api::auth::require_token(State, headers) -> Result<(), AppError>`：解析 `Authorization: Bearer <raw>` → `tokens::verify`；失败 401 `{"error":{"code":401,"message":"无效的 API Token"}}`
  - 中间件应用：`/api/*` 全部要求 Bearer **或** admin session（`auth::require_admin_or_token`，T10 已建，扩展为 Bearer 校验）
  - 端点（方法/路径/入参/出参）：
    ```
    POST   /api/posts              body: {title, content_md, slug?, excerpt?, status?, category_id?, tags?[]} → 201 {data: Post}
    GET    /api/posts?page=&page_size=&status=&category=&tag=   → {data:{items:[Post],total}}
    GET    /api/posts/{id}          → {data: Post}（404 时 {error}）
    PATCH  /api/posts/{id}          body 部分字段 → {data: Post}
    DELETE /api/posts/{id}          → 204
    POST   /api/moments             body: {content, attachment_ids?[]} → 201 {data: Moment}
    DELETE /api/moments/{id}        → 204
    POST   /api/uploads             multipart files（Bearer 或 session）→ {data:[Attachment]}
    GET    /api/categories          → {data:[Category]}
    POST   /api/categories          body {name, slug?, sort_order?} → 201
    PATCH  /api/categories/{id}     → {data}
    DELETE /api/categories/{id}     → 204
    GET    /api/stats/summary?from=&to=  → {data: StatsSummary}
    GET    /api/backup              → zip 二进制（Content-Disposition: attachment）
    GET    /api/health              → {data:{status:"ok"}}
    ```
  - 请求体校验：title 非空、content_md 存在；status 枚举校验；category_id 必须存在否则 400；错误统一 `AppError`
  - `docs/ai-integration.md`：给 agent 的接口手册（base URL、鉴权、每接口示例 curl、发布文章最佳实践、错误处理建议），后续 MCP server 基于此文档实现

- [ ] **Step 1: 写失败测试**

`tests/api_auth.rs`：

```rust
mod common;
use common::test_app;
use hancic::db;
use hancic::services::tokens;
use tower::ServiceExt;
use axum::http::{Request, StatusCode, header};
use axum::body::Body;

#[tokio::test]
async fn api_requires_token() {
    let (app, pool) = test_app("api-auth").await;
    let res = app.clone().oneshot(Request::builder()
        .method("POST").uri("/api/posts")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"title":"x","content_md":"y"}"#)).unwrap()).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn valid_token_creates_post() {
    let (app, pool) = test_app("api-auth2").await;
    let (_tok, raw) = tokens::generate(&pool, "test").await.unwrap();
    let res = app.oneshot(Request::builder()
        .method("POST").uri("/api/posts")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {raw}"))
        .body(Body::from(r#"{"title":"API 文章","content_md":"来自 agent","status":"published","tags":["ai"]}"#)).unwrap()).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let body: serde_json::Value = serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 1024*1024).await.unwrap()).unwrap();
    assert_eq!(body["data"]["title"], "API 文章");
}
```

`tests/api_posts.rs`：全 CRUD 走查（含 404、校验 400、moments、categories、stats）。

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --test api_auth --test api_posts`
Expected: 编译失败（`api::auth`、端点缺失）

- [ ] **Step 3: 实现 API 层**

`src/api/auth.rs`：从 `headers.get(AUTHORIZATION)` 提取 Bearer → `tokens::verify`。`require_admin_or_token` 放入 `api::mod.rs`：`(session, headers)` → session 通过则 Ok，否则 Bearer 校验。`/api/uploads` 改造为同时接受两者（T10 只接受 session，本任务补 Bearer）。`posts.rs`：`Json<serde_json::Value>` 入参用 `serde_json::from_value` 映射到 `NewPost`/`UpdatePost`（手写反序列化，字段可选），响应 `StatusCode::CREATED + Json(json!({"data": post}))`。Post 序列化时 `content_md` 保留（AI 需要原文），`excerpt/published_at/views` 一并返回。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --test api_auth --test api_posts`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat: REST API（Bearer 鉴权 + 统一 JSON）与 AI 集成指南"
```

---

## 阶段 F：备份与迁移（T22–T23）

### Task 22: 备份与恢复（全量 zip 导出/导入）

**Files:**
- Create: `src/services/backup.rs`, `src/admin/backup.rs`, `assets/admin_templates/backup.html`
- Modify: `src/admin/mod.rs`
- Test: `tests/backup_restore.rs`

**Interfaces:**
- Consumes: T2（Db）、T21（`GET /api/backup` 端点）
- Produces:
  - `services::backup::export_all(data_dir: &Path, out_zip: &Path) -> Result<BackupReport>`：用 `VACUUM INTO '.../hancic.db'` 生成一致性快照到临时目录 → 打 zip：`config.toml`（若无则跳过）、`hancic.db`、`uploads/`、`themes/`、`meta.json`（`{version, exported_at}`）→ 返回 `{path, size, counts}`
  - `services::backup::restore(data_dir: &Path, zip_path: &Path) -> Result<RestoreReport>`：校验 zip 含 `hancic.db` + `meta.json` → 现有数据目录改名 `data_dir.bak-<ts>` 后解包 `uploads/`、`themes/`、`config.toml` → 替换 db → 返回报告；提示重启生效
  - 路由：
    ```
    GET  /admin/backup         页面（导出按钮 + 上传恢复表单）
    POST /admin/backup/export  下载 zip（Content-Disposition）
    POST /admin/backup/restore 上传 zip → 校验 → 执行 → 结果页
    ```
  - 安全：restore 上传 zip ≤ 500MB；zip 解包路径穿越防护（`ZipFile::enclosed_name()`）

- [ ] **Step 1: 写失败测试**

`tests/backup_restore.rs`：

```rust
mod common;
use common::test_config;
use hancic::db;
use hancic::services::{backup, posts, moments};
use hancic::models::PostStatus;

#[tokio::test]
async fn export_then_restore_roundtrip() {
    let cfg = test_config("backup");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    posts::create_post(&pool, posts::NewPost {
        title: "备份文章".into(), content_md: "内容".into(), excerpt: None, slug: None,
        status: PostStatus::Published, post_type: hancic::models::PostType::Post,
        category_id: None, tags: vec!["backup".into()],
    }).await.unwrap();
    moments::create_moment(&pool, "备份说说", &[]).await.unwrap();

    let zip_path = cfg.data_dir.join("backup.zip");
    backup::export_all(&cfg.data_dir, &zip_path).await.unwrap();

    // 破坏数据
    posts::delete_post(&pool, 1).await.unwrap();

    backup::restore(&cfg.data_dir, &zip_path).await.unwrap();
    let pool2 = db::init(&cfg.data_dir).await.unwrap();
    assert!(posts::get_post(&pool2, 1).await.unwrap().is_some());
    assert_eq!(moments::list_moments(&pool2, 1, 10).await.unwrap().1, 1);
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --test backup_restore`
Expected: 编译失败（`services::backup` 不存在）

- [ ] **Step 3: 实现**

`VACUUM INTO` 快照（`sqlx::query("VACUUM INTO ?")` 不接受参数——**实现**：`sqlx::raw_sql(&format!("VACUUM INTO '{}'", path.display().replace('\'', "''")))`）。zip 用 `zip::ZipWriter`，目录递归；解包用 `ZipArchive` + `enclosed_name()` 校验。restore 前置 `db.close()` 语义：SQLite 连接池在 restore 前 drop（handler 内 `std::mem::drop(pool.clone())` 不够——连接池关闭需 `pool.close().await`；SQLite WAL 下重命名数据目录可能失败，**实现**：restore 先 `CHECKPOINT`（`PRAGMA wal_checkpoint(TRUNCATE)`）再操作，做完重开池）。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --test backup_restore`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat: 全量备份与恢复"
```

---

### Task 23: Halo Markdown zip 迁移导入

**Files:**
- Create: `src/services/migrate.rs`, `src/admin/migrate.rs`, `assets/admin_templates/migrate.html`
- Modify: `src/admin/mod.rs`
- Test: `tests/migrate_halo.rs`

**Interfaces:**
- Consumes: T3（posts/taxonomy）、T10（uploads 落库）
- Produces:
  - `migrate::parse_front_matter(raw: &str) -> Result<FrontMatter>`：解析 `---\n...\n---`（title/date/categories/tags/slug/type）；无 front-matter 时 title=首行 `# `，slug=文件名
  - `migrate::import_halo_zip(db, data_dir, zip_path, download_images: bool) -> Result<ImportReport>`
    `ImportReport { posts_created: usize, posts_skipped: usize, categories: usize, tags: usize, images_downloaded: usize, images_failed: usize, failures: Vec<String> }`
  - 流程：遍历 zip 内 `*.md` → front-matter → 分类/标签 `ensure` → `create_post`（slug 冲突自动后缀；status=published，published_at=date）→ 正文图片：收集 `![alt](url)`（http/https 或 zip 内相对路径）→ 下载/解包 → `uploads::save_bytes`（失败记 failures）→ 替换正文 URL 为 `/uploads/<path>` → 更新文章
  - 路由：
    ```
    GET  /admin/migrate       页（上传 + 选项 checkbox 下载图片）
    POST /admin/migrate       上传 zip → 执行 → 报告页
    ```
  - zip 限制 ≤ 500MB；只处理根/任意层级 `.md`；跳过空文件

- [ ] **Step 1: 写失败测试**

`tests/migrate_halo.rs`：

```rust
mod common;
use common::test_config;
use hancic::db;
use hancic::services::migrate;
use hancic::services::posts;

fn build_fixture_zip(path: &std::path::Path) {
    // 用 zip crate 生成：posts/hello.md（front-matter: title=你好 halo, date=2024-01-01,
    // categories=[技术], tags=[rust]; 正文含 ![](https://example.com/x.png) 与 ![](local/1.png)）
    // 外加 assets/local/1.png（真实 PNG bytes）
}

#[tokio::test]
async fn parse_front_matter_basic() {
    let raw = "---\ntitle: 你好\ndate: 2024-01-01 10:00:00\ncategories: [技术]\ntags: [rust, 博客]\n---\n# 正文";
    let fm = migrate::parse_front_matter(raw).unwrap();
    assert_eq!(fm.title, "你好");
    assert_eq!(fm.categories, vec!["技术"]);
}

#[tokio::test]
async fn import_creates_posts_and_local_images() {
    let cfg = test_config("migrate");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    let zip_path = cfg.data_dir.join("halo-export.zip");
    build_fixture_zip(&zip_path);
    // download_images=false：zip 内相对图片也导入（local/1.png 走解包），外部 URL 跳过
    let report = migrate::import_halo_zip(&pool, &cfg.data_dir, &zip_path, false).await.unwrap();
    assert_eq!(report.posts_created, 1);
    let p = posts::get_post_by_slug(&pool, "hello").await.unwrap().unwrap();
    assert_eq!(p.title, "你好 halo");
    // 本地图片已入库并替换 URL
    assert!(p.content_md.contains("/uploads/"));
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --test migrate_halo`
Expected: 编译失败（`services::migrate` 不存在）

- [ ] **Step 3: 实现**

front-matter 解析手写（`---` 行分隔 + 逐行 `key: value`，值支持 `[a, b]` 列表与引号），不引入 frontmatter crate（格式简单）。图片替换用正则或手写扫描（`![...](...)` 模式）。外部 URL 下载用 reqwest（超时 30s，失败记入报告）；zip 内相对路径图片从 zip 读取 bytes 走 `save_bytes`。`download_images=false` 时：zip 内图片仍导入，仅跳过外部 URL。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --test migrate_halo`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat: Halo Markdown zip 迁移导入"
```

---

## 阶段 G：端到端测试（T24）

### Task 24: Playwright 端到端（含 375px 移动端）

**Files:**
- Create: `e2e/package.json`, `e2e/playwright.config.ts`, `e2e/tests/{publish-post.spec.ts, publish-moment.spec.ts, paste-upload.spec.ts, mobile.spec.ts}`, `e2e/global-setup.ts`（起 dev server + 初始化数据）
- Modify: `.gitignore`（`/e2e/node_modules`、`/e2e/test-results`、`/e2e/playwright-report`）

**Interfaces:**
- Consumes: 全部前台/后台/上传/API
- Produces: CI 可跑的 E2E 套件

- [ ] **Step 1: 初始化 Playwright**

```bash
cd e2e && npm init -y && npm i -D @playwright/test && npx playwright install chromium
```

- [ ] **Step 2: 写配置与全局 setup**

`e2e/playwright.config.ts`：

```ts
import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "./tests",
  timeout: 60_000,
  globalSetup: "./global-setup.ts",
  use: { baseURL: "http://127.0.0.1:8090" },
  projects: [
    { name: "desktop", use: { viewport: { width: 1280, height: 800 } } },
    { name: "mobile", use: { viewport: { width: 375, height: 667 } } },
  ],
});
```

`e2e/global-setup.ts`：`cargo run -- ../data-e2e`（独立数据目录 + 测试密码 + 已知 Token）后台起服，返回 `process.env` 句柄；`global-teardown` 关进程。（开发期数据目录 `/data-e2e` 由 setup 脚本预置：设置密码 + 生成 token + 1 篇示例文章。）

- [ ] **Step 3: 写四个用例**

`publish-post.spec.ts`（桌面）：登录 → 新建文章 → Vditor 输入标题/正文 → 存草稿 → 发布 → 前台 `/post/{slug}` 可见正文与阅读量。断言使用 `page.get_by_role`/`get_by_text`。

`publish-moment.spec.ts`：登录 → 后台说说 → 输入文字 → 选图（`setInputFiles` 上传 fixture 图）→ 发布 → 前台 `/moments` 见文字与 `<img>`。

`paste-upload.spec.ts`：编辑页 → 用 `page.keyboard` 粘贴（ClipboardEvent 注入 PNG 到 Vditor）→ 断言出现 `<img src="/uploads/...">` 且附件库+1。（Vditor 粘贴上传依赖 `upload.handler`，用 `page.evaluate` 构造 ClipboardEvent 或 `page.dispatchEvent`。）

`mobile.spec.ts`（mobile project）：375px 下打开首页（导航可展开）→ `/moments` 发一条带图说说 → 新建并发布带图文章 → 文章页截图断言正文不横向溢出（`scrollWidth <= clientWidth`）。

- [ ] **Step 4: 本地跑通**

Run: `cd e2e && npx playwright test`
Expected: 4 个 spec 全绿（两个 project 共 7 个用例）

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "test: Playwright 端到端（含移动端视口）"
```

---

## 阶段 H：交付（T25–T27）

### Task 25: Docker 镜像与 docker-compose

**Files:**
- Create: `Dockerfile`, `docker-compose.yaml`, `.dockerignore`
- Test: 本地 `docker build` + `docker compose up` 冒烟（curl /api/health + 首页 200）

**Interfaces:**
- Consumes: 全部（产物稳定）
- Produces:
  - `Dockerfile`：多阶段——builder `rust:1.88-alpine`（`apk add musl-dev`，`--release`）→ runner `alpine:3.20`（`apk add ca-certificates`；拷贝二进制 + `assets/`（含 vendor 与 ip2region.xdb）+ 默认 `config.toml`）；`EXPOSE 8090`；`ENV` 数据目录 `/data`；`VOLUME /data`；非 root 用户（`adduser -D hancic`）；`HEALTHCHECK` 用 `wget -qO- http://127.0.0.1:8090/api/health || exit 1`（alpine busybox wget）
  - `docker-compose.yaml`：单服务 `hancic`，端口映射 `8090:8090`，卷 `hancic-data:/data`，`restart: unless-stopped`，healthcheck，`environment: RUST_LOG=info`
  - 镜像大小目标：二进制 + assets ≤ 100MB（预期 25–40MB）；`docker build` 本机验证 + `docker images` 记录大小

- [ ] **Step 1: 写 Dockerfile**

```dockerfile
FROM rust:1.88-alpine AS builder
RUN apk add --no-cache musl-dev
WORKDIR /build
COPY . .
RUN cargo build --release

FROM alpine:3.20
RUN apk add --no-cache ca-certificates && adduser -D hancic
WORKDIR /app
COPY --from=builder /build/target/release/hancic /app/hancic
COPY --from=builder /build/assets /app/assets
COPY config.example.toml /app/config.toml
USER hancic
ENV RUST_LOG=info
VOLUME /data
EXPOSE 8090
HEALTHCHECK --interval=30s --timeout=5s --retries=3 \
  CMD wget -qO- http://127.0.0.1:8090/api/health || exit 1
CMD ["/app/hancic", "/data/config.toml"]
```

（`config.example.toml` 的 `data_dir="/data"` 与 `active_theme="default"`；`assets/` 需含 ip2region.xdb——fetch-assets 产物提交入库后构建可用。）

- [ ] **Step 2: 写 compose 与 .dockerignore**

`.dockerignore`：`target/`、`data*/`、`.git/`、`e2e/`、`tests/`、`*.md`（保留 docs 可去掉）。

```yaml
services:
  hancic:
    build: .
    image: ghcr.io/angryshark708/hancic:latest
    container_name: hancic
    restart: unless-stopped
    ports:
      - "8090:8090"
    volumes:
      - hancic-data:/data
    environment:
      - RUST_LOG=info
volumes:
  hancic-data:
```

- [ ] **Step 3: 冒烟验证**

```bash
docker build -t hancic:test .
docker images | grep hancic   # 记录大小，断言 < 100MB
docker compose up -d
sleep 3 && curl -s http://127.0.0.1:8090/api/health   # {"data":{"status":"ok"}}
curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:8090/   # 200
docker compose down
```

- [ ] **Step 4: 提交**

```bash
git add -A && git commit -m "build: Docker 多阶段镜像与 docker-compose"
```

---

### Task 26: GitHub Actions CI

**Files:**
- Create: `.github/workflows/ci.yaml`

**Interfaces:**
- Consumes: T24（e2e）、T25（docker）
- Produces: push main 时自动跑 test + lint + build + docker push ghcr.io/angryshark708/hancic

- [ ] **Step 1: 写 workflow**

`.github/workflows/ci.yaml`（要点）：

```yaml
name: CI
on:
  push:
    branches: [main]
  pull_request:

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { components: rustfmt, clippy }
      - run: bash scripts/fetch-assets.sh
      - run: cargo fmt --check
      - run: cargo clippy -- -D warnings
      - run: cargo test
      - uses: actions/setup-node@v4
        with: { node-version: 22 }
      - run: cd e2e && npm ci && npx playwright install --with-deps chromium
      - run: cd e2e && npx playwright test
  docker:
    needs: test
    if: github.ref == 'refs/heads/main'
    runs-on: ubuntu-latest
    permissions: { contents: read, packages: write }
    steps:
      - uses: actions/checkout@v4
      - uses: docker/setup-buildx-action@v3
      - uses: docker/login-action@v3
        with: { registry: ghcr.io, username: ${{ github.actor }}, password: ${{ secrets.GITHUB_TOKEN }} }
      - uses: docker/build-push-action@v6
        with:
          push: true
          tags: ghcr.io/angryshark708/hancic:latest
          cache-from: type=gha
          cache-to: type=gha,mode=max
```

- [ ] **Step 2: 本地模拟验证**

Run: `cargo fmt --check && cargo clippy -- -D warnings && cargo test`
Expected: 全绿；推送后 GitHub Actions 上 test job 含 e2e 也绿

- [ ] **Step 3: 提交**

```bash
git add -A && git commit -m "ci: GitHub Actions（test/lint/docker 推送）"
```

---

### Task 27: 上海主机上线与 hancic.site 切换

**Files:**
- Create: `scripts/deploy-sh.sh`, `docs/deploy-sh.md`（上线手册）
- 操作对象：上海 172.81.241.149（4C8G，无 GitHub 访问）、北京 nginx、hancic.site 域名

**Interfaces:**
- Consumes: T25/T26（镜像）、T23（数据迁移）
- 目标：镜像在 sh 主机可拉取并运行；真实数据从 Halo 迁入；北京 nginx 切流量；验收 7 条标准

- [ ] **Step 1: 打通 sh 主机镜像拉取**

`docs/deploy-sh.md` 记录（沿用现有 server-monitor/my-nginx 先例）：
- 方案 A：sh 主机 docker daemon 配置国内镜像加速（阿里云/腾讯云容器镜像服务）拉 `ghcr.io/...`（ghcr 国内直连不稳，**优先方案 B**）
- 方案 B：经 usa（170.106.103.36，可访问 GitHub）中转——在 usa 上 `docker pull ghcr.io/angryshark708/hancic:latest && docker save | ssh sh 'docker load'`，或 sh 上直接 `docker pull` usa 上起的内网 registry
- `scripts/deploy-sh.sh`：`docker compose pull && docker compose up -d hancic && curl health`，留 `--rollback`（切回上一版镜像）分支

- [ ] **Step 2: 预置数据目录与首启**

sh 主机 `/data/hancic/`：`config.toml`（data_dir=/data）、`themes/default/`（从镜像 cp 或用卷绑定挂载本地目录）、首启访问 `/admin/setup` 设置密码（或通过 `/api` + 首启 token——**选定**：设置密码后到 `/admin/tokens` 生成部署 token 备用）。

- [ ] **Step 3: 真实数据迁移**

1. 旧 Halo 后台装 halo-plugin-export-md → 导出 zip 下载到本机
2. 新系统后台 `/admin/migrate` 上传 → 勾选下载图片 → 执行 → 检查报告（文章数/图片数）
3. 核对：hancic 文章数与 Halo 一致；抽查 3 篇长文图片可显示；分类/标签完整；Halo 瞬间（说说）有导出则迁移，否则手动补录 `/moments`
4. `/admin/backup` 做一次全量导出留档

- [ ] **Step 4: 北京 nginx 切换与验收**

北京 nginx 反代 `hancic.site` 根路径由 `8090`（halo）改指 sh 主机 `8090`（hancic 新端口，与 halo 同端口会冲突——**选定**：hancic 起在 `8091`，nginx 指 8091；halo 容器先不关，回滚只需 nginx 指回 8090）：
```nginx
location / {
    proxy_pass http://172.81.241.149:8091;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
}
```
验收（对照 §17 成功标准）：
1. `docker stats` 记录 hancic 空闲内存（预期 < 50MB）✓ ≤100MB；镜像大小已由 T25 记录 ✓ ≤100MB
2. 375px 移动端发带图说说、发带图文章 ✓（T24 自动化覆盖 + 真机抽查）
3. 桌面粘贴截图自动上传 ✓
4. Halo zip 导入数量一致、图片可用 ✓（Step 3）
5. agent 用 Token 发文章 ✓（curl 实测 + 记入 ai-integration.md 示例）
6. 地区统计正确、图片体积显著变小 ✓（后台抽查 + 附件库对比原图）
7. hancic.site 文章/说说/导航可用 ✓（URL 逐条抽查 + 站内搜索）

观察 1–2 周后，确认无问题再 `docker stop halo`；出问题 `nginx` 指回 8090 即回滚。

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "deploy: 上海主机上线手册与部署脚本"
```

---

## Self-Review

对照设计文档 §1–§17 逐项核查：

- **§3 架构**（单体/模块划分）→ 文件树总览 + T1–T23 覆盖全部模块（config/db/models/web/admin/api/themes/upload/stats/migrate/backup）✓
- **§4 数据模型** → T2 迁移 10 表 + FTS5 + 触发器；字段与关联一致（posts.type/category 单分类、moment_attachments 有序）✓
- **§5 前台页面** → T7 全部路由 + 移动端/亮暗/lightbox；T11 moments；T9 search；T7 错误页 ✓
- **§6 后台 11 模块** → T12 仪表盘、T13 文章、T14 说说、T15 附件、T16 分类标签、T17 设置、T18 主题、T19 统计、T20 Token、T22 备份、T23 迁移 ✓（全部含二次确认/验证/错误提示）
- **§7 编辑体验** → T13 Vditor IR + 30s 自动保存 + 粘贴/拖拽上传（T10 handler）✓；T14 朋友圈式发布框 ✓
- **§8 REST API** → T21 全部端点 + 统一 JSON + Bearer + AI 集成指南；MCP 后期（明确不做）✓
- **§9 主题系统** → T6 契约/discover/tera + T18 切换/预览 + 主题开发指南 ✓；换主题重启生效 ✓
- **§10 上传与媒体** → T10 白名单/上限/UUID/压缩/GIF 跳过/视频不转码 ✓
- **§11 统计** → T8 记录+解析、T19 展示下钻清理、T21 API ✓
- **§12 迁移** → T23 zip 解析/图片下载/报告 ✓
- **§13 部署 CI** → T25 多阶段 alpine（≤100MB）、T26 CI（test/lint/docker push）、T27 中转拉取/nginx 切换/备份 ✓
- **§14 测试** → 单元（各服务测试）+ axum 集成（tests/*）+ Playwright 375px（T24）✓
- **§15 安全** → T4 argon2、T5 session/CSRF/限流、T20 token 哈希+subtle、T10 白名单/UUID、参数化 SQL（服务层全程 ?）、pulldown-cmark 默认安全 ✓
- **§16 里程碑** → v1 全范围，不做项未混入 ✓
- **§17 验收** → T27 Step 4 逐条映射 ✓

**占位符扫描**：无 TBD/TODO；两处实现细节以"以实际 API 为准/按编译错误修正"标注（tower-sessions store 构造、ip2region crate 接口），均为外部库版本差异导致的必要弹性，核心契约已固定。
**类型一致性**：`test_app(tag) -> (Router, Db, PathBuf)` 在 T7 修正后沿用（T8/T9/T10/T11/T21 一致）；`AppState { config, db, tera, tera_admin, theme_dir, ip_searcher, login_limiter }` 自 T6 起一致；`NewPost/UpdatePost/PostListOptions` 在 T3 定义后 T13/T21 沿用；`StatsSummary/top_posts/by_region` 在 T8 定义后 T19/T21 沿用 ✓
**任务顺序**：T21 依赖 T22 服务层（`GET /api/backup`）→ 已在 T21 标注顺序修正；`markdown::render` 提前到 T6 建（主题 filter 引用）→ 已在 T6 修正 ✓
