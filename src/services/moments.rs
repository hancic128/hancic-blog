//! 说说服务：说说 CRUD、附件关联（按 sort_order）与按天分组。
//!
//! 关联删除由 DB 外键 `ON DELETE CASCADE` 承担：删除说说时
//! `moment_attachments` 对应行自动清空，`attachments` 记录本身保留。

use crate::db::Db;
use crate::error::AppError;
use crate::models::{Attachment, AttachmentKind, Moment};
use chrono::{DateTime, Utc};
use sqlx::FromRow;
use std::str::FromStr;

#[derive(FromRow)]
struct MomentRow {
    id: i64,
    content: String,
    created_at: DateTime<Utc>,
    like_count: i64,
}

impl From<MomentRow> for Moment {
    fn from(r: MomentRow) -> Self {
        Moment {
            id: r.id,
            content: r.content,
            created_at: r.created_at,
            like_count: r.like_count,
        }
    }
}

const MOMENT_COLUMNS: &str = "id, content, created_at, like_count";

/// 创建说说：事务内 INSERT moments + 按数组序 INSERT moment_attachments。
/// `attachment_ids` 为空数组时仅建说说本体。
pub async fn create_moment(
    db: &Db,
    content: &str,
    attachment_ids: &[i64],
) -> Result<Moment, AppError> {
    let mut tx = db.begin().await?;
    let id = sqlx::query("INSERT INTO moments(content) VALUES (?)")
        .bind(content)
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();
    for (i, aid) in attachment_ids.iter().enumerate() {
        sqlx::query(
            "INSERT INTO moment_attachments(moment_id, attachment_id, sort_order) \
             VALUES (?, ?, ?)",
        )
        .bind(id)
        .bind(aid)
        .bind(i as i64)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    get_moment(db, id)
        .await?
        .ok_or_else(|| AppError::Internal("说说创建后读取失败".into()))
}

pub async fn get_moment(db: &Db, id: i64) -> Result<Option<Moment>, AppError> {
    let sql = format!("SELECT {MOMENT_COLUMNS} FROM moments WHERE id = ?");
    let row = sqlx::query_as::<_, MomentRow>(&sql)
        .bind(id)
        .fetch_optional(db)
        .await?;
    Ok(row.map(Moment::from))
}

/// 分页列出说说（倒序）与总数；`page` 从 1 起。
pub async fn list_moments(
    db: &Db,
    month: Option<&str>,
    asc: bool,
    q: Option<&str>,
    page: i64,
    page_size: i64,
) -> Result<(Vec<Moment>, i64), AppError> {
    let page = page.max(1);
    // 动态组合 where：月份前缀 + 内容关键词，均按绑定参数处理
    let mut where_parts: Vec<&str> = Vec::new();
    let mut binds: Vec<String> = Vec::new();
    if let Some(m) = month.filter(|m| !m.is_empty()) {
        where_parts.push("substr(created_at, 1, 7) = ?");
        binds.push(m.to_string());
    }
    if let Some(kw) = q.map(str::trim).filter(|s| !s.is_empty()) {
        where_parts.push("content LIKE ?");
        binds.push(format!("%{kw}%"));
    }
    let where_sql = if where_parts.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", where_parts.join(" AND "))
    };

    let total: i64 = {
        let count_sql = format!("SELECT COUNT(*) FROM moments{where_sql}");
        let mut count_q = sqlx::query_scalar::<_, i64>(&count_sql);
        for b in &binds {
            count_q = count_q.bind(b);
        }
        count_q.fetch_one(db).await?
    };
    let order = if asc { "ASC" } else { "DESC" };
    let sql = format!(
        "SELECT {MOMENT_COLUMNS} FROM moments{where_sql} \
         ORDER BY created_at {order}, id {order} LIMIT ? OFFSET ?"
    );
    let mut q = sqlx::query_as::<_, MomentRow>(&sql);
    for b in &binds {
        q = q.bind(b);
    }
    let rows = q
        .bind(page_size)
        .bind((page - 1) * page_size)
        .fetch_all(db)
        .await?;
    Ok((rows.into_iter().map(Moment::from).collect(), total))
}

/// 说说月份列表：有说说的月份按 `YYYY-MM` 去重倒序（侧栏筛选用）。
pub async fn month_list(db: &Db) -> Result<Vec<String>, AppError> {
    let months: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT substr(created_at, 1, 7) AS m FROM moments
         ORDER BY m DESC",
    )
    .fetch_all(db)
    .await?;
    Ok(months)
}

/// 说说附件（按 sort_order 升序），返回附件与排序值。
pub async fn list_moment_attachments(
    db: &Db,
    moment_id: i64,
) -> Result<Vec<(Attachment, i64)>, AppError> {
    let rows = sqlx::query_as::<_, MomentAttachmentRow>(
        "SELECT a.id, a.uuid_name, a.orig_name, a.mime, a.size, a.kind, a.path, \
                a.created_at, ma.sort_order \
         FROM moment_attachments ma JOIN attachments a ON a.id = ma.attachment_id \
         WHERE ma.moment_id = ? ORDER BY ma.sort_order, a.id",
    )
    .bind(moment_id)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            let sort = r.sort_order;
            (r.into_attachment(), sort)
        })
        .collect())
}

/// 全局搜索：按关键词 LIKE 匹配说说内容（moments 无 FTS，量小走 LIKE）。
/// 转义 `%`/`_`/`\`，返回最近 `limit` 条 `(id, content, 日期 YYYY-MM-DD)`。
pub async fn search_moments(
    db: &Db,
    q: &str,
    limit: i64,
) -> Result<Vec<(i64, String, String)>, AppError> {
    let escaped = q.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    let like = format!("%{escaped}%");
    Ok(sqlx::query_as::<_, (i64, String, String)>(
        "SELECT id, content, substr(created_at, 1, 10) AS d
         FROM moments
         WHERE content LIKE ? ESCAPE '\\'
         ORDER BY created_at DESC
         LIMIT ?",
    )
    .bind(like)
    .bind(limit)
    .fetch_all(db)
    .await?)
}

/// 删除说说：moment_attachments 由外键级联清空，attachments 记录保留。
pub async fn delete_moment(db: &Db, moment_id: i64) -> Result<(), AppError> {
    let r = sqlx::query("DELETE FROM moments WHERE id = ?")
        .bind(moment_id)
        .execute(db)
        .await?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound("说说不存在".into()));
    }
    Ok(())
}

/// 按站点时区把 moments 换算为本地日期（`YYYY-MM-DD`）分组，组序保持输入顺序。
/// 调用方应传入 `list_moments` 的输出（时间倒序），同一天记录连续。
pub async fn group_by_day(
    db: &Db,
    moments: Vec<Moment>,
) -> Result<Vec<(String, Vec<Moment>)>, AppError> {
    let tz = crate::services::timezone::site_timezone(db).await;
    let mut groups: Vec<(String, Vec<Moment>)> = Vec::new();
    for m in moments {
        let day = m.created_at.with_timezone(&tz).format("%Y-%m-%d").to_string();
        match groups.last_mut() {
            Some((d, list)) if *d == day => list.push(m),
            _ => groups.push((day, vec![m])),
        }
    }
    Ok(groups)
}

#[derive(FromRow)]
struct MomentAttachmentRow {
    id: i64,
    uuid_name: String,
    orig_name: String,
    mime: String,
    size: i64,
    kind: String,
    path: String,
    created_at: DateTime<Utc>,
    sort_order: i64,
}

impl MomentAttachmentRow {
    fn into_attachment(self) -> Attachment {
        Attachment {
            id: self.id,
            uuid_name: self.uuid_name,
            orig_name: self.orig_name,
            mime: self.mime,
            size: self.size,
            kind: AttachmentKind::from_str(&self.kind).unwrap_or_default(),
            path: self.path,
            created_at: self.created_at,
        }
    }
}

/// 更新说说内容；不存在返回 false。
pub async fn update_content(db: &Db, id: i64, content: &str) -> Result<bool, AppError> {
    let r = sqlx::query("UPDATE moments SET content = ? WHERE id = ?")
        .bind(content)
        .bind(id)
        .execute(db)
        .await?;
    Ok(r.rows_affected() > 0)
}

/// 更新说说内容并**整体重建附件关联**：删除该说说原有关联，按 `attachment_ids`
/// 顺序重新插入（编辑页提交完整保留列表：移除项=删除，新增项=新增，替换=删旧加新）。
/// 返回 false 表示说说不存在。
pub async fn update_moment_with_attachments(
    db: &Db,
    id: i64,
    content: &str,
    attachment_ids: &[i64],
) -> Result<bool, AppError> {
    let mut tx = db.begin().await?;
    let r = sqlx::query("UPDATE moments SET content = ? WHERE id = ?")
        .bind(content)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    if r.rows_affected() == 0 {
        return Ok(false);
    }
    sqlx::query("DELETE FROM moment_attachments WHERE moment_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    for (i, aid) in attachment_ids.iter().enumerate() {
        sqlx::query(
            "INSERT INTO moment_attachments(moment_id, attachment_id, sort_order) \
             VALUES (?, ?, ?)",
        )
        .bind(id)
        .bind(aid)
        .bind(i as i64)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(true)
}
