//! 设置服务：settings 表的键值读取与写入。

use crate::db::Db;
use crate::error::AppError;
use std::collections::HashMap;

pub async fn get(db: &Db, key: &str) -> Result<Option<String>, AppError> {
    let value: Option<String> =
        sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(db)
            .await?;
    Ok(value)
}

pub async fn set(db: &Db, key: &str, value: &str) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO settings(key, value) VALUES (?, ?) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn get_many(db: &Db, keys: &[&str]) -> Result<HashMap<String, String>, AppError> {
    let mut map = HashMap::new();
    for key in keys {
        if let Some(v) = get(db, key).await? {
            map.insert(key.to_string(), v);
        }
    }
    Ok(map)
}

pub async fn all(db: &Db) -> Result<HashMap<String, String>, AppError> {
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM settings")
        .fetch_all(db)
        .await?;
    Ok(rows.into_iter().collect())
}
