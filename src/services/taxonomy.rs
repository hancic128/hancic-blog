//! 分类与标签服务：标准 CRUD，slug 唯一约束，标签按 slug 去重。

use crate::db::Db;
use crate::error::AppError;
use crate::models::{Category, Tag};
use crate::services::posts::slugify;

pub async fn list_categories(db: &Db) -> Result<Vec<Category>, AppError> {
    let rows = sqlx::query_as::<_, Category>(
        "SELECT id, slug, name, sort_order FROM categories ORDER BY sort_order, id",
    )
    .fetch_all(db)
    .await?;
    Ok(rows)
}

pub async fn create_category(
    db: &Db,
    name: &str,
    slug: &str,
    sort_order: i64,
) -> Result<Category, AppError> {
    if get_category_by_slug(db, slug).await?.is_some() {
        return Err(AppError::Conflict("分类 slug 已存在".into()));
    }
    let id = sqlx::query("INSERT INTO categories(slug, name, sort_order) VALUES (?, ?, ?)")
        .bind(slug)
        .bind(name)
        .bind(sort_order)
        .execute(db)
        .await?
        .last_insert_rowid();
    Ok(Category {
        id,
        slug: slug.to_string(),
        name: name.to_string(),
        sort_order,
    })
}

pub async fn update_category(
    db: &Db,
    id: i64,
    name: &str,
    slug: &str,
    sort_order: i64,
) -> Result<Category, AppError> {
    let r = sqlx::query("UPDATE categories SET name = ?, slug = ?, sort_order = ? WHERE id = ?")
        .bind(name)
        .bind(slug)
        .bind(sort_order)
        .bind(id)
        .execute(db)
        .await?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound("分类不存在".into()));
    }
    Ok(Category {
        id,
        slug: slug.to_string(),
        name: name.to_string(),
        sort_order,
    })
}

pub async fn delete_category(db: &Db, id: i64) -> Result<(), AppError> {
    let r = sqlx::query("DELETE FROM categories WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound("分类不存在".into()));
    }
    Ok(())
}

pub async fn get_category_by_slug(db: &Db, slug: &str) -> Result<Option<Category>, AppError> {
    let row = sqlx::query_as::<_, Category>(
        "SELECT id, slug, name, sort_order FROM categories WHERE slug = ?",
    )
    .bind(slug)
    .fetch_optional(db)
    .await?;
    Ok(row)
}

/// 按 id 读取分类（API 层 PATCH 合并字段与 category_id 存在性校验用）。
pub async fn get_category_by_id(db: &Db, id: i64) -> Result<Option<Category>, AppError> {
    let row = sqlx::query_as::<_, Category>(
        "SELECT id, slug, name, sort_order FROM categories WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(db)
    .await?;
    Ok(row)
}

pub async fn list_tags(db: &Db) -> Result<Vec<Tag>, AppError> {
    let rows = sqlx::query_as::<_, Tag>("SELECT id, slug, name FROM tags ORDER BY id")
        .fetch_all(db)
        .await?;
    Ok(rows)
}

/// 按名称（slug 统一小写）查找标签，不存在则创建。
pub async fn ensure_tag(db: &Db, name: &str) -> Result<Tag, AppError> {
    let slug = slugify(name).await;
    if let Some(t) = get_tag_by_slug(db, &slug).await? {
        return Ok(t);
    }
    let id = sqlx::query("INSERT INTO tags(slug, name) VALUES (?, ?)")
        .bind(&slug)
        .bind(name)
        .execute(db)
        .await?
        .last_insert_rowid();
    Ok(Tag {
        id,
        slug,
        name: name.to_string(),
    })
}

async fn get_tag_by_slug(db: &Db, slug: &str) -> Result<Option<Tag>, AppError> {
    let row = sqlx::query_as::<_, Tag>("SELECT id, slug, name FROM tags WHERE slug = ?")
        .bind(slug)
        .fetch_optional(db)
        .await?;
    Ok(row)
}

pub async fn delete_tag(db: &Db, id: i64) -> Result<(), AppError> {
    let r = sqlx::query("DELETE FROM tags WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound("标签不存在".into()));
    }
    Ok(())
}
