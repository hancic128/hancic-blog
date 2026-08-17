//! 专栏服务：标准 CRUD（仿分类），slug 唯一约束。
//!
//! 专栏是文章的系列合集（一篇文章属于 0/1 个专栏，`posts.column_id`）；
//! 删除专栏依赖 `ON DELETE SET NULL` 保留关联文章。

use crate::db::Db;
use crate::error::AppError;
use crate::models::Column;
use crate::services::posts::slugify;
use std::collections::HashMap;

pub async fn list_columns(db: &Db) -> Result<Vec<Column>, AppError> {
    let rows = sqlx::query_as::<_, Column>(
        "SELECT id, slug, name, sort_order, description FROM columns ORDER BY sort_order ASC, id DESC",
    )
    .fetch_all(db)
    .await?;
    Ok(rows)
}

pub async fn create_column(
    db: &Db,
    name: &str,
    slug: &str,
    sort_order: i64,
    description: &str,
) -> Result<Column, AppError> {
    if get_column_by_slug(db, slug).await?.is_some() {
        return Err(AppError::Conflict("专栏 slug 已存在".into()));
    }
    let id = sqlx::query(
        "INSERT INTO columns(slug, name, sort_order, description) VALUES (?, ?, ?, ?)",
    )
    .bind(slug)
    .bind(name)
    .bind(sort_order)
    .bind(description)
    .execute(db)
    .await?
    .last_insert_rowid();
    Ok(Column {
        id,
        slug: slug.to_string(),
        name: name.to_string(),
        sort_order,
        description: description.to_string(),
    })
}

/// 改名/改描述保留原 slug（避免前台 /column/{slug} 链接失效）；slug 仅创建时生成。
pub async fn update_column(db: &Db, id: i64, name: &str, description: &str) -> Result<Column, AppError> {
    let r = sqlx::query("UPDATE columns SET name = ?, description = ? WHERE id = ?")
        .bind(name)
        .bind(description)
        .bind(id)
        .execute(db)
        .await?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound("专栏不存在".into()));
    }
    let row = sqlx::query_as::<_, Column>(
        "SELECT id, slug, name, sort_order, description FROM columns WHERE id = ?",
    )
    .bind(id)
    .fetch_one(db)
    .await?;
    Ok(row)
}

pub async fn delete_column(db: &Db, id: i64) -> Result<(), AppError> {
    let r = sqlx::query("DELETE FROM columns WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound("专栏不存在".into()));
    }
    Ok(())
}

pub async fn get_column_by_slug(db: &Db, slug: &str) -> Result<Option<Column>, AppError> {
    let row = sqlx::query_as::<_, Column>(
        "SELECT id, slug, name, sort_order, description FROM columns WHERE slug = ?",
    )
    .bind(slug)
    .fetch_optional(db)
    .await?;
    Ok(row)
}

/// 按 id 查专栏（API 层校验用）。
pub async fn get_column_by_id(db: &Db, id: i64) -> Result<Option<Column>, AppError> {
    let row = sqlx::query_as::<_, Column>(
        "SELECT id, slug, name, sort_order, description FROM columns WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(db)
    .await?;
    Ok(row)
}

/// 各专栏下已发布普通文章数（未发布/页面不计），专栏页排序与后台计数用。
pub async fn count_columns_posts(db: &Db) -> Result<HashMap<i64, i64>, AppError> {
    let rows: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT c.id, COUNT(p.id) FROM columns c \
         LEFT JOIN posts p ON p.column_id = c.id \
            AND p.status = 'published' AND p.post_type = 'post' \
         GROUP BY c.id",
    )
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().collect())
}

/// 专栏 slug 生成：空名回退默认；与分类/标签同一套 slugify。
pub async fn slug_for(name: &str) -> String {
    let s = slugify(name).await;
    if s.is_empty() { "column".to_string() } else { s }
}

/// 专栏卡片拖拽排序：按传入 id 顺序重写 sort_order（1..n；新建专栏保持 0 排最前）。
pub async fn reorder_columns(db: &Db, ids: &[i64]) -> Result<(), AppError> {
    for (idx, id) in ids.iter().enumerate() {
        sqlx::query("UPDATE columns SET sort_order = ? WHERE id = ?")
            .bind((idx as i64) + 1)
            .bind(id)
            .execute(db)
            .await?;
    }
    Ok(())
}
