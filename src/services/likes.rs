use crate::db::Db;
use crate::error::AppError;
use crate::models::LikeContentType;
use crate::services::{moments, posts};
use chrono::Utc;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqliteExecutor};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const LIKE_RATE_LIMIT_WINDOW_SECS: i64 = 60;
const LIKE_RATE_LIMIT_MAX_REQUESTS: i64 = 6;

#[derive(Debug, Clone, Serialize)]
pub struct LikeStatus {
    pub liked: bool,
    pub like_count: i64,
}

#[derive(Clone)]
pub struct LikeRateLimiter {
    inner: Arc<Mutex<HashMap<String, (i64, i64)>>>,
}

impl LikeRateLimiter {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn now_secs() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    pub fn check(&self, key: &str) -> bool {
        let mut map = self.inner.lock().expect("like limiter 锁可用");
        let now = Self::now_secs();
        match map.get_mut(key) {
            None => {
                map.insert(key.to_string(), (1, now));
                true
            }
            Some(entry) if now - entry.1 > LIKE_RATE_LIMIT_WINDOW_SECS => {
                *entry = (1, now);
                true
            }
            Some(entry) if entry.0 < LIKE_RATE_LIMIT_MAX_REQUESTS => {
                entry.0 += 1;
                true
            }
            Some(_) => false,
        }
    }
}

impl Default for LikeRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

pub fn hash_client_hint(raw: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    let bytes = hasher.finalize();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub async fn like_status(
    db: &Db,
    target: LikeContentType,
    content_id: i64,
    visitor_id: &str,
) -> Result<LikeStatus, AppError> {
    ensure_target_exists(db, target, content_id).await?;
    let liked: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM content_likes WHERE content_type = ? AND content_id = ? AND visitor_id = ?",
    )
    .bind(target.to_str())
    .bind(content_id)
    .bind(visitor_id)
    .fetch_optional(db)
    .await?;
    let like_count = current_like_count_db(db, target, content_id).await?;
    Ok(LikeStatus {
        liked: liked.is_some(),
        like_count,
    })
}

pub async fn toggle_like(
    db: &Db,
    target: LikeContentType,
    content_id: i64,
    visitor_id: &str,
    ip_hash: &str,
    ua_hash: &str,
) -> Result<LikeStatus, AppError> {
    ensure_target_exists(db, target, content_id).await?;
    let mut tx = db.begin().await?;

    let exists: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM content_likes WHERE content_type = ? AND content_id = ? AND visitor_id = ?",
    )
    .bind(target.to_str())
    .bind(content_id)
    .bind(visitor_id)
    .fetch_optional(&mut *tx)
    .await?;

    let liked = if let Some(id) = exists {
        sqlx::query("DELETE FROM content_likes WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        bump_like_count(&mut *tx, target, content_id, -1).await?;
        false
    } else {
        let inserted = sqlx::query(
            "INSERT OR IGNORE INTO content_likes(content_type, content_id, visitor_id, ip_hash, ua_hash) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(target.to_str())
        .bind(content_id)
        .bind(visitor_id)
        .bind(ip_hash)
        .bind(ua_hash)
        .execute(&mut *tx)
        .await?
        .rows_affected()
            > 0;
        if inserted {
            bump_like_count(&mut *tx, target, content_id, 1).await?;
        }
        true
    };

    let like_count = recount_like_count(&mut *tx, target, content_id).await?;
    set_like_count(&mut *tx, target, content_id, like_count).await?;
    tx.commit().await?;
    Ok(LikeStatus { liked, like_count })
}

pub async fn recent_like_count(db: &Db, days: i64) -> Result<i64, AppError> {
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(days)).to_rfc3339();
    let total = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM content_likes WHERE created_at >= ?",
    )
    .bind(cutoff)
    .fetch_one(db)
    .await?;
    Ok(total)
}

/// 按日点赞数（站点时区自然日分组，与仪表盘阅读趋势横轴一致）。
/// `from`/`to` 为 `YYYY-MM-DD`（站点时区日期），缺省不设限；返回 [(日期, 点赞数)] 升序。
pub async fn daily_like_count(
    db: &Db,
    from: Option<&str>,
    to: Option<&str>,
    tz: &chrono_tz::Tz,
) -> Result<Vec<(String, i64)>, AppError> {
    let (lower, upper) =
        crate::services::timezone::local_day_utc_bounds(from, to, tz);
    let mut conds: Vec<String> = Vec::new();
    let mut binds: Vec<String> = Vec::new();
    if let Some(f) = lower {
        conds.push("created_at >= ?".to_string());
        binds.push(f);
    }
    if let Some(t) = upper {
        conds.push("created_at < ?".to_string());
        binds.push(t);
    }
    let where_sql = if conds.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conds.join(" AND "))
    };
    // 逐条按站点时区归属自然日（SQLite 无 IANA 时区转换）
    let sql = format!(
        "SELECT created_at FROM content_likes {where_sql} ORDER BY created_at"
    );
    let mut q = sqlx::query(&sql);
    for b in &binds {
        q = q.bind(b);
    }
    let rows = q.fetch_all(db).await?;
    let mut by_day: Vec<(String, i64)> = Vec::new();
    for row in rows {
        let raw: String = row.get(0);
        let parsed = chrono::NaiveDateTime::parse_from_str(
            raw.trim_end_matches('Z'),
            "%Y-%m-%dT%H:%M:%S",
        );
        let Ok(ndt) = parsed else { continue };
        let utc_dt =
            chrono::DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc);
        let day = utc_dt.with_timezone(tz).format("%Y-%m-%d").to_string();
        match by_day.last_mut() {
            Some((d, c)) if *d == day => *c += 1,
            _ => by_day.push((day, 1)),
        }
    }
    Ok(by_day)
}

async fn ensure_target_exists(
    db: &Db,
    target: LikeContentType,
    content_id: i64,
) -> Result<(), AppError> {
    let exists = match target {
        LikeContentType::Post => posts::get_post(db, content_id).await?.is_some(),
        LikeContentType::Moment => moments::get_moment(db, content_id).await?.is_some(),
    };
    if exists {
        Ok(())
    } else {
        let label = match target {
            LikeContentType::Post => "文章",
            LikeContentType::Moment => "说说",
        };
        Err(AppError::NotFound(format!("{label}不存在")))
    }
}

async fn bump_like_count<'e, E>(
    executor: E,
    target: LikeContentType,
    content_id: i64,
    delta: i64,
) -> Result<(), AppError>
where
    E: SqliteExecutor<'e>,
{
    let table = match target {
        LikeContentType::Post => "posts",
        LikeContentType::Moment => "moments",
    };
    let sql = format!(
        "UPDATE {table} SET like_count = MAX(0, like_count + ?) WHERE id = ?"
    );
    sqlx::query(&sql)
        .bind(delta)
        .bind(content_id)
        .execute(executor)
        .await?;
    Ok(())
}

async fn current_like_count<'e, E>(
    executor: E,
    target: LikeContentType,
    content_id: i64,
) -> Result<i64, AppError>
where
    E: SqliteExecutor<'e>,
{
    let table = match target {
        LikeContentType::Post => "posts",
        LikeContentType::Moment => "moments",
    };
    let sql = format!("SELECT like_count FROM {table} WHERE id = ?");
    let count = sqlx::query_scalar::<_, i64>(&sql)
        .bind(content_id)
        .fetch_one(executor)
        .await?;
    Ok(count)
}

async fn current_like_count_db(
    db: &Db,
    target: LikeContentType,
    content_id: i64,
) -> Result<i64, AppError> {
    current_like_count(db, target, content_id).await
}

async fn recount_like_count<'e, E>(
    executor: E,
    target: LikeContentType,
    content_id: i64,
) -> Result<i64, AppError>
where
    E: SqliteExecutor<'e>,
{
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM content_likes WHERE content_type = ? AND content_id = ?",
    )
    .bind(target.to_str())
    .bind(content_id)
    .fetch_one(executor)
    .await?;
    Ok(count)
}

async fn set_like_count<'e, E>(
    executor: E,
    target: LikeContentType,
    content_id: i64,
    like_count: i64,
) -> Result<(), AppError>
where
    E: SqliteExecutor<'e>,
{
    let table = match target {
        LikeContentType::Post => "posts",
        LikeContentType::Moment => "moments",
    };
    let sql = format!("UPDATE {table} SET like_count = ? WHERE id = ?");
    sqlx::query(&sql)
        .bind(like_count)
        .bind(content_id)
        .execute(executor)
        .await?;
    Ok(())
}
