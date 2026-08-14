use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::path::Path;
use std::str::FromStr;

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

pub async fn init(data_dir: &Path) -> Result<Db, sqlx::Error> {
    std::fs::create_dir_all(data_dir)
        .map_err(|e| sqlx::Error::Configuration(Box::new(e)))?;
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
    sqlx::raw_sql(MIGRATION_002).execute(&pool).await?;
    ensure_column_id(&pool).await?;
    ensure_column_description(&pool).await?;
    seed_default_settings(&pool).await?;
    Ok(pool)
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

/// posts 表加 `column_id`（幂等：已有列则跳过；SQLite ADD COLUMN 支持带
/// REFERENCES 的 NULL 列，删除专栏时关联文章自动置空）。
async fn ensure_column_id(pool: &Db) -> Result<(), sqlx::Error> {
    let has: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM pragma_table_info('posts') WHERE name = 'column_id'",
    )
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

async fn seed_default_settings(pool: &Db) -> Result<(), sqlx::Error> {
    let defaults: &[(&str, &str)] = &[
        ("site_name", "寒蝉 Hancic"),
        ("site_desc", ""),
        ("site_nav", r#"[{"type":"home","label":"首页","url":"/"},{"type":"articles","label":"文章","url":"/archives"},{"type":"moments","label":"说说","url":"/moments"},{"type":"link","label":"关于","url":"/about"}]"#),
        ("site_social", r#"{}"#),
        ("active_theme", "default"),
        ("theme_mode", "auto"),        // auto | light | dark
        ("timezone", "Asia/Shanghai"),
        // 页脚/友情链接/悬浮联系方式卡片（空默认，后台设置页填）
        ("footer_text", ""),
        ("friend_links", r#"[]"#),
        ("contact_enabled", "0"),
        ("contact_email", ""),
        // 站点 Logo / 社交图标（空默认，后台设置页填）
        ("site_logo", ""),
        ("social_logos", r#"{}"#),
    ];
    for (k, v) in defaults {
        sqlx::query("INSERT OR IGNORE INTO settings(key, value) VALUES (?, ?)")
            .bind(k).bind(v).execute(pool).await?;
    }
    Ok(())
}
