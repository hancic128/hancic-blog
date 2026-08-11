use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::path::Path;
use std::str::FromStr;

pub type Db = SqlitePool;

const MIGRATION_001: &str = include_str!("../migrations/001_init.sql");

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
    seed_default_settings(&pool).await?;
    Ok(pool)
}

async fn seed_default_settings(pool: &Db) -> Result<(), sqlx::Error> {
    let defaults: &[(&str, &str)] = &[
        ("site_name", "寒蝉 Hancic"),
        ("site_desc", ""),
        ("site_nav", r#"[{"label":"首页","url":"/"},{"label":"文章","url":"/archives"},{"label":"说说","url":"/moments"},{"label":"关于","url":"/about"}]"#),
        ("site_social", r#"{}"#),
        ("active_theme", "default"),
        ("theme_mode", "auto"),        // auto | light | dark
        ("timezone", "Asia/Shanghai"),
        // 页脚/友情链接/悬浮联系方式卡片（空默认，后台设置页填）
        ("footer_text", ""),
        ("friend_links", r#"[]"#),
        ("contact_enabled", "0"),
        ("contact_email", ""),
        ("contact_qr", r#"{}"#),
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
