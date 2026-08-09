//! 说说服务：说说 CRUD、附件关联（按 sort_order）与按天分组。
//!
//! 关联删除由 DB 外键 `ON DELETE CASCADE` 承担：删除说说时
//! `moment_attachments` 对应行自动清空，`attachments` 记录本身保留。

use crate::db::Db;
use crate::error::AppError;
use crate::models::{Attachment, AttachmentKind, Moment};
use crate::services::settings;
use chrono::{DateTime, Utc};
use sqlx::FromRow;
use std::str::FromStr;

/// 默认时区：settings.timezone 缺失或非法时的回退值。
const DEFAULT_TZ: &str = "Asia/Shanghai";

#[derive(FromRow)]
struct MomentRow {
    id: i64,
    content: String,
    created_at: DateTime<Utc>,
}

impl From<MomentRow> for Moment {
    fn from(r: MomentRow) -> Self {
        Moment {
            id: r.id,
            content: r.content,
            created_at: r.created_at,
        }
    }
}

const MOMENT_COLUMNS: &str = "id, content, created_at";

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
    page: i64,
    page_size: i64,
) -> Result<(Vec<Moment>, i64), AppError> {
    let total: i64 = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM moments")
        .fetch_one(db)
        .await?;
    let page = page.max(1);
    let sql = format!(
        "SELECT {MOMENT_COLUMNS} FROM moments ORDER BY created_at DESC, id DESC LIMIT ? OFFSET ?"
    );
    let rows = sqlx::query_as::<_, MomentRow>(&sql)
        .bind(page_size)
        .bind((page - 1) * page_size)
        .fetch_all(db)
        .await?;
    Ok((rows.into_iter().map(Moment::from).collect(), total))
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
    let tz = site_timezone(db).await;
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

/// 读取 settings.timezone 并解析为 `chrono_tz::Tz`；缺失/解析失败回退默认时区。
async fn site_timezone(db: &Db) -> chrono_tz::Tz {
    let raw = settings::get(db, "timezone").await.ok().flatten();
    raw.as_deref()
        .and_then(|s| chrono_tz::Tz::from_str(s).ok())
        .unwrap_or_else(|| {
            chrono_tz::Tz::from_str(DEFAULT_TZ).expect("默认时区 Asia/Shanghai 应合法")
        })
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
