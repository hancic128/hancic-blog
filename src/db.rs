use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::path::Path;
use std::str::FromStr;
use uuid::Uuid;

pub type Db = SqlitePool;

const MIGRATION_001: &str = include_str!("../migrations/001_init.sql");

/// 专栏表（幂等，重复执行无副作用）。
const MIGRATION_002: &str = r#"
CREATE TABLE IF NOT EXISTS columns (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  slug       TEXT NOT NULL UNIQUE,
  name       TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0
);
"#;

/// 徒步轨迹表（幂等，重复执行无副作用）。
///
/// 存元数据与统计：GPX 原文件与完整坐标 JSON 落在运行时 `data/trails/`（gitignore），
/// `simplified` 为抽稀后坐标 JSON `[[lat,lon],...]`（总览地图直接嵌入，不读文件）。
/// `sha256` 为 GPX 文件内容哈希（上传去重用，由 `ensure_trail_sha256` 补齐）。
const MIGRATION_003: &str = r#"
CREATE TABLE IF NOT EXISTS trails (
  id                INTEGER PRIMARY KEY AUTOINCREMENT,
  name              TEXT NOT NULL,
  description       TEXT NOT NULL DEFAULT '',
  file_path         TEXT NOT NULL,
  started_at        TEXT,
  distance_m        REAL,
  elevation_gain_m  REAL,
  elevation_loss_m  REAL,
  moving_seconds    INTEGER,
  avg_speed_kmh     REAL,
  max_elevation_m   REAL,
  min_elevation_m   REAL,
  start_lat         REAL,
  start_lon         REAL,
  end_lat           REAL,
  end_lon           REAL,
  simplified        TEXT NOT NULL DEFAULT '[]',
  point_count       INTEGER NOT NULL DEFAULT 0,
  created_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  updated_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);
"#;

pub async fn init(data_dir: &Path) -> Result<Db, sqlx::Error> {
    std::fs::create_dir_all(data_dir).map_err(|e| sqlx::Error::Configuration(Box::new(e)))?;
    let db_path = data_dir.join("hancic.db");
    let opts = SqliteConnectOptions::from_str(&format!("sqlite://{}", db_path.display()))?
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(std::time::Duration::from_secs(5))
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(10)
        .connect_with(opts)
        .await?;
    sqlx::raw_sql(MIGRATION_001).execute(&pool).await?;
    sqlx::raw_sql(MIGRATION_002).execute(&pool).await?;
    sqlx::raw_sql(MIGRATION_003).execute(&pool).await?;
    ensure_post_uuid(&pool).await?;
    ensure_column_id(&pool).await?;
    ensure_column_sort(&pool).await?;
    ensure_column_description(&pool).await?;
    ensure_trail_sha256(&pool).await?;
    ensure_like_schema(&pool).await?;
    ensure_page_view_source(&pool).await?;
    ensure_analytics_schema(&pool).await?;
    seed_default_settings(&pool).await?;
    Ok(pool)
}

/// trails 表加 `sha256` 列（幂等：GPX 文件内容哈希，上传去重用）。
/// 附部分唯一索引：既有旧行（NULL）不受影响，新行同哈希拒绝。
async fn ensure_trail_sha256(pool: &Db) -> Result<(), sqlx::Error> {
    let has: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM pragma_table_info('trails') WHERE name = 'sha256'")
            .fetch_one(pool)
            .await?;
    if has.0 == 0 {
        sqlx::raw_sql("ALTER TABLE trails ADD COLUMN sha256 TEXT")
            .execute(pool)
            .await?;
    }
    sqlx::raw_sql(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_trails_sha256 \
         ON trails(sha256) WHERE sha256 IS NOT NULL",
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// columns 表加 `description` 列（幂等：专栏卡片总览页展示用）。
async fn ensure_column_description(pool: &Db) -> Result<(), sqlx::Error> {
    let has: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM pragma_table_info('columns') WHERE name = 'description'",
    )
    .fetch_one(pool)
    .await?;
    if has.0 == 0 {
        sqlx::raw_sql("ALTER TABLE columns ADD COLUMN description TEXT NOT NULL DEFAULT ''")
            .execute(pool)
            .await?;
    }
    Ok(())
}

/// posts 表加 `uuid`（幂等：旧库补列，历史文章自动回填随机 UUID，并建唯一索引）。
async fn ensure_post_uuid(pool: &Db) -> Result<(), sqlx::Error> {
    let has: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM pragma_table_info('posts') WHERE name = 'uuid'")
            .fetch_one(pool)
            .await?;
    if has.0 == 0 {
        sqlx::raw_sql("ALTER TABLE posts ADD COLUMN uuid TEXT")
            .execute(pool)
            .await?;
    }
    let ids: Vec<(i64,)> = sqlx::query_as("SELECT id FROM posts WHERE uuid IS NULL OR uuid = ''")
        .fetch_all(pool)
        .await?;
    for (id,) in ids {
        sqlx::query("UPDATE posts SET uuid = ? WHERE id = ?")
            .bind(Uuid::new_v4().to_string())
            .bind(id)
            .execute(pool)
            .await?;
    }
    sqlx::raw_sql("CREATE UNIQUE INDEX IF NOT EXISTS idx_posts_uuid ON posts(uuid)")
        .execute(pool)
        .await?;
    Ok(())
}

/// posts 表加 `column_id`（幂等：已有列则跳过；SQLite ADD COLUMN 支持带
/// REFERENCES 的 NULL 列，删除专栏时关联文章自动置空）。
async fn ensure_column_id(pool: &Db) -> Result<(), sqlx::Error> {
    let has: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM pragma_table_info('posts') WHERE name = 'column_id'")
            .fetch_one(pool)
            .await?;
    if has.0 == 0 {
        sqlx::raw_sql(
            "ALTER TABLE posts ADD COLUMN column_id INTEGER REFERENCES columns(id) ON DELETE SET NULL",
        )
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// posts 表加 `column_sort`（幂等：专栏内文章自定义顺序，0=未手动排序）。
async fn ensure_column_sort(pool: &Db) -> Result<(), sqlx::Error> {
    let has: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM pragma_table_info('posts') WHERE name = 'column_sort'",
    )
    .fetch_one(pool)
    .await?;
    if has.0 == 0 {
        sqlx::raw_sql("ALTER TABLE posts ADD COLUMN column_sort INTEGER NOT NULL DEFAULT 0")
            .execute(pool)
            .await?;
    }
    Ok(())
}

async fn ensure_like_schema(pool: &Db) -> Result<(), sqlx::Error> {
    sqlx::raw_sql(
        r#"
CREATE TABLE IF NOT EXISTS content_likes (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  content_type TEXT NOT NULL,
  content_id INTEGER NOT NULL,
  visitor_id TEXT NOT NULL,
  ip_hash TEXT NOT NULL DEFAULT '',
  ua_hash TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
  UNIQUE(content_type, content_id, visitor_id)
);
CREATE INDEX IF NOT EXISTS idx_content_likes_target ON content_likes(content_type, content_id);
"#,
    )
    .execute(pool)
    .await?;

    let has_post_like: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM pragma_table_info('posts') WHERE name = 'like_count'")
            .fetch_one(pool)
            .await?;
    if has_post_like.0 == 0 {
        sqlx::raw_sql("ALTER TABLE posts ADD COLUMN like_count INTEGER NOT NULL DEFAULT 0")
            .execute(pool)
            .await?;
    }

    let has_moment_like: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM pragma_table_info('moments') WHERE name = 'like_count'",
    )
    .fetch_one(pool)
    .await?;
    if has_moment_like.0 == 0 {
        sqlx::raw_sql("ALTER TABLE moments ADD COLUMN like_count INTEGER NOT NULL DEFAULT 0")
            .execute(pool)
            .await?;
    }

    // 迁移历史点赞数据到 content_likes（幂等：检测迁移标记记录，存在则跳过）
    let migrated: Option<(i64,)> = sqlx::query_as(
        "SELECT COUNT(*) FROM content_likes WHERE visitor_id LIKE 'migrated-%' LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    if migrated.is_none() {
        // 迁移文章历史点赞
        sqlx::raw_sql(
            "INSERT INTO content_likes (content_type, content_id, visitor_id, ip_hash, ua_hash, created_at)
             SELECT 'post', id, 'migrated-' || id, '', '', datetime('now')
             FROM posts WHERE like_count > 0",
        )
        .execute(pool)
        .await?;
        // 迁移说说历史点赞
        sqlx::raw_sql(
            "INSERT INTO content_likes (content_type, content_id, visitor_id, ip_hash, ua_hash, created_at)
             SELECT 'moment', id, 'migrated-' || id, '', '', datetime('now')
             FROM moments WHERE like_count > 0",
        )
        .execute(pool)
        .await?;
    }

    Ok(())
}

/// page_views 表加 `source` 列（幂等：旧库补列，跳转来源分类，历史数据置 'other'）。
async fn ensure_page_view_source(pool: &Db) -> Result<(), sqlx::Error> {
    let has: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM pragma_table_info('page_views') WHERE name = 'source'",
    )
    .fetch_one(pool)
    .await?;
    if has.0 == 0 {
        sqlx::raw_sql("ALTER TABLE page_views ADD COLUMN source TEXT NOT NULL DEFAULT 'other'")
            .execute(pool)
            .await?;
    }
    Ok(())
}

/// page_views 表加访问分析维度列（幂等：旧库逐列补齐）。
///
/// 新增列：
/// - `device` / `browser` / `os`：UA 解析结果（见 `services::ua`）
/// - `ref_domain` / `ref_path`：Referer 域名与路径（见 `services::referer`）
/// - `search_query`：搜索引擎关键词
/// - `visitor_id`：访客指纹（IP+UA 哈希），用于独立访客数与新老访客判定
///
/// 历史数据回填：`page_views` 早已存了 `ua` / `referer` / `ip`，故这些维度
/// 可从既有列重新解析出来——升级后**旧流量也有完整分析**，而非从升级那一刻
/// 起才有数据。回填按 id 升序分批处理，避免一次性载入全表占满内存。
async fn ensure_analytics_schema(pool: &Db) -> Result<(), sqlx::Error> {
    for (name, ddl) in [
        (
            "device",
            "ALTER TABLE page_views ADD COLUMN device TEXT NOT NULL DEFAULT ''",
        ),
        (
            "browser",
            "ALTER TABLE page_views ADD COLUMN browser TEXT NOT NULL DEFAULT ''",
        ),
        (
            "os",
            "ALTER TABLE page_views ADD COLUMN os TEXT NOT NULL DEFAULT ''",
        ),
        (
            "ref_domain",
            "ALTER TABLE page_views ADD COLUMN ref_domain TEXT NOT NULL DEFAULT ''",
        ),
        (
            "ref_path",
            "ALTER TABLE page_views ADD COLUMN ref_path TEXT NOT NULL DEFAULT ''",
        ),
        (
            "search_query",
            "ALTER TABLE page_views ADD COLUMN search_query TEXT NOT NULL DEFAULT ''",
        ),
        (
            "visitor_id",
            "ALTER TABLE page_views ADD COLUMN visitor_id TEXT NOT NULL DEFAULT ''",
        ),
    ] {
        let has: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM pragma_table_info('page_views') WHERE name = ?")
                .bind(name)
                .fetch_one(pool)
                .await?;
        if has.0 == 0 {
            sqlx::raw_sql(ddl).execute(pool).await?;
        }
    }

    sqlx::raw_sql(
        "CREATE INDEX IF NOT EXISTS idx_page_views_device ON page_views(device);
         CREATE INDEX IF NOT EXISTS idx_page_views_visitor ON page_views(visitor_id);
         CREATE INDEX IF NOT EXISTS idx_page_views_ref_domain ON page_views(ref_domain);",
    )
    .execute(pool)
    .await?;

    backfill_analytics_rows(pool).await
}

/// 回填历史 `page_views` 的分析维度。
///
/// 只处理「维度仍为空」的行：写入路径已解析过的新行不重复计算。
/// 爬虫行只记 `device = 'bot'`（浏览器/系统留空），不做进一步解析——
/// 爬虫的 UA 没有分析价值，省下的是实打实的回填开销。
async fn backfill_analytics_rows(pool: &Db) -> Result<(), sqlx::Error> {
    const BATCH: i64 = 500;
    loop {
        // 取一批待回填行（device 为空）；LIMIT 内按 id 升序保证推进
        let rows: Vec<(i64, String, String, String)> = sqlx::query_as(
            "SELECT id, ua, referer, ip FROM page_views
             WHERE device = '' ORDER BY id ASC LIMIT ?",
        )
        .bind(BATCH)
        .fetch_all(pool)
        .await?;
        if rows.is_empty() {
            break;
        }
        for (id, ua, referer, ip) in &rows {
            let info = crate::services::ua::parse(ua);
            if info.device_type == crate::services::ua::DeviceType::Bot {
                sqlx::query("UPDATE page_views SET device = 'bot' WHERE id = ?")
                    .bind(id)
                    .execute(pool)
                    .await?;
                continue;
            }
            let r = crate::services::referer::parse(referer);
            let visitor = visitor_fingerprint(ip, ua);
            sqlx::query(
                "UPDATE page_views
                 SET device = ?, browser = ?, os = ?, ref_domain = ?, ref_path = ?,
                     search_query = ?, visitor_id = ?
                 WHERE id = ?",
            )
            .bind(info.device_type.as_str())
            .bind(&info.browser)
            .bind(&info.os)
            .bind(&r.domain)
            .bind(&r.path)
            .bind(&r.search_query)
            .bind(&visitor)
            .bind(id)
            .execute(pool)
            .await?;
        }
        // 本批已全部置为非空，下一轮查询条件 `device = ''` 不再命中这批；
        // 若返回行数小于 BATCH 说明剩余不足一批，下一轮自然返回空并退出。
    }
    Ok(())
}

/// 访客指纹：IP + UA 的 SHA-256 哈希（截取 16 字节十六进制）。
///
/// 存哈希而非明文 IP：既能区分访客（算独立访客数），又不把原始 IP
/// 暴露在分析表里。同一访客换 UA 会被算作两个访客——这是有意的保守选择，
/// 避免用 UA 拼接做「跨设备合并」带来的误判。
pub fn visitor_fingerprint(ip: &str, ua: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(ip.trim().as_bytes());
    hasher.update(b"\n");
    hasher.update(ua.trim().as_bytes());
    let digest = hasher.finalize();
    digest[..16].iter().map(|b| format!("{b:02x}")).collect()
}

async fn seed_default_settings(pool: &Db) -> Result<(), sqlx::Error> {
    let defaults: &[(&str, &str)] = &[
        ("site_name", "我的博客"),
        ("site_desc", ""),
        (
            "site_nav",
            r#"[{"type":"home","label":"首页","url":"/"},{"type":"articles","label":"文章","url":"/archives"},{"type":"column","label":"专栏","url":"/columns"},{"type":"trail","label":"轨迹","url":"/trails"},{"type":"moments","label":"说说","url":"/moments"},{"type":"link","label":"关于","url":"/about"}]"#,
        ),
        ("site_social", r#"{}"#),
        ("active_theme", "default"),
        ("theme_mode", "auto"), // auto | light | dark
        ("timezone", "Asia/Shanghai"),
        // 页脚/友情链接/悬浮联系方式卡片（空默认，后台设置页填）
        ("footer_text", ""),
        ("friend_links", r#"[]"#),
        ("contact_enabled", "0"),
        ("contact_email", ""),
        // 站点 Logo / 社交图标（空默认，后台设置页填）
        ("site_logo", ""),
        ("social_logos", r#"{}"#),
        // 前台日期展示格式：datetime = YYYY-MM-DD HH:MM（默认）/ date = 仅日期
        ("date_format", "datetime"),
    ];
    for (k, v) in defaults {
        sqlx::query("INSERT OR IGNORE INTO settings(key, value) VALUES (?, ?)")
            .bind(k)
            .bind(v)
            .execute(pool)
            .await?;
    }
    Ok(())
}
