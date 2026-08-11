//! 分类与标签服务：标准 CRUD，slug 唯一约束，标签按 slug 去重。

use crate::db::Db;
use crate::error::AppError;
use crate::models::{Category, Tag};
use crate::services::posts::slugify;
use std::collections::HashMap;

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

/// 各分类的已发布普通文章数（未发布/页面不计；未分类文章不归属任何分类）。
pub async fn count_categories_posts(db: &Db) -> Result<HashMap<i64, i64>, AppError> {
    let rows: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT c.id, COUNT(p.id) FROM categories c \
         LEFT JOIN posts p ON p.category_id = c.id \
            AND p.status = 'published' AND p.post_type = 'post' \
         GROUP BY c.id",
    )
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().collect())
}

/// 各标签的已发布普通文章数（未发布/页面不计）。
pub async fn count_tags_posts(db: &Db) -> Result<HashMap<i64, i64>, AppError> {
    let rows: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT t.id, COUNT(p.id) FROM tags t \
         LEFT JOIN post_tags pt ON pt.tag_id = t.id \
         LEFT JOIN posts p ON p.id = pt.post_id \
            AND p.status = 'published' AND p.post_type = 'post' \
         GROUP BY t.id",
    )
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().collect())
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

/// 某分类下所有文章使用的标签（去重，按名排序）。
pub async fn tags_of_category(db: &Db, category_id: i64) -> Result<Vec<Tag>, AppError> {
    let rows: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT DISTINCT t.id, t.slug, t.name
         FROM tags t
         JOIN post_tags pt ON pt.tag_id = t.id
         JOIN posts p ON p.id = pt.post_id
         WHERE p.category_id = ? AND p.status = 'published'
         ORDER BY t.name",
    )
    .bind(category_id)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, slug, name)| Tag { id, slug, name })
        .collect())
}
