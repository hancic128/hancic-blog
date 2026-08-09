//! 文章服务：slug 生成、文章 CRUD、列表分页、相邻文章与标签管理。

use crate::db::Db;
use crate::error::AppError;
use crate::models::{Post, PostStatus, PostType, Tag};
use chrono::{DateTime, SecondsFormat, Utc};
use sqlx::FromRow;
use sqlx::Row;
use std::str::FromStr;

/// 动态 SQL 的绑定值（`SqliteArgumentValue` 在 sqlx 0.8.6 不支持 `Type`，故用本地枚举）。
enum BindVal {
    Text(String),
    Int(i64),
}

pub struct NewPost {
    pub title: String,
    pub content_md: String,
    pub excerpt: Option<String>,
    pub slug: Option<String>,
    pub status: PostStatus,
    pub post_type: PostType,
    pub category_id: Option<i64>,
    /// 标签名列表
    pub tags: Vec<String>,
}

pub struct UpdatePost {
    /// None = 不变；slug/tags 特殊：Some(_) 即替换
    pub title: Option<String>,
    pub content_md: Option<String>,
    pub excerpt: Option<String>,
    pub slug: Option<String>,
    pub status: Option<PostStatus>,
    pub post_type: Option<PostType>,
    pub category_id: Option<i64>,
    pub tags: Option<Vec<String>>,
}

pub struct PostListOptions {
    pub status: Option<PostStatus>,
    /// None = 全部类型（向后兼容，T3 调用不受影响）
    pub post_type: Option<PostType>,
    pub category_slug: Option<String>,
    pub tag_slug: Option<String>,
    pub page: i64,
    pub page_size: i64,
}

/// 数据库行结构：枚举字段以 String 存取，经 `to_str`/`from_str` 与模型互转。
#[derive(FromRow)]
struct PostRow {
    id: i64,
    slug: String,
    title: String,
    content_md: String,
    excerpt: String,
    status: String,
    post_type: String,
    published_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    views: i64,
    category_id: Option<i64>,
}

impl From<PostRow> for Post {
    fn from(r: PostRow) -> Self {
        Post {
            id: r.id,
            slug: r.slug,
            title: r.title,
            content_md: r.content_md,
            excerpt: r.excerpt,
            status: PostStatus::from_str(&r.status).unwrap_or_default(),
            post_type: PostType::from_str(&r.post_type).unwrap_or_default(),
            published_at: r.published_at,
            created_at: r.created_at,
            updated_at: r.updated_at,
            views: r.views,
            category_id: r.category_id,
        }
    }
}

const POST_COLUMNS: &str = "id, slug, title, content_md, excerpt, status, post_type, \
    published_at, created_at, updated_at, views, category_id";

pub async fn slugify(input: &str) -> String {
    let s = input.trim().to_lowercase();
    // 保留 CJK 与字母数字，其余空白/标点转 '-'；连续分隔符合并、去首尾 '-'
    let mut out = String::new();
    let mut pending_dash = false;
    for ch in s.chars() {
        if ch.is_alphanumeric() {
            out.push(ch);
            pending_dash = false;
        } else if !out.is_empty() && !pending_dash {
            out.push('-');
            pending_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

async fn unique_slug(db: &Db, base: &str) -> Result<String, AppError> {
    if get_post_by_slug(db, base).await?.is_none() {
        return Ok(base.to_string());
    }
    for i in 2..1000 {
        let candidate = format!("{base}-{i}");
        if get_post_by_slug(db, &candidate).await?.is_none() {
            return Ok(candidate);
        }
    }
    Err(AppError::Internal("slug 冲突过多".into()))
}

pub async fn create_post(db: &Db, input: NewPost) -> Result<Post, AppError> {
    let slug_base = input
        .slug
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| input.title.clone());
    let slug = unique_slug(db, &slugify(&slug_base).await).await?;
    let excerpt = match input.excerpt.clone().filter(|s| !s.trim().is_empty()) {
        Some(e) => e,
        None => excerpt_of(&input.content_md).await,
    };
    let status = input.status.to_str();
    let post_type = input.post_type.to_str();
    let published_at = if input.status == PostStatus::Published {
        Some(Utc::now())
    } else {
        None
    };
    let id = sqlx::query(
        "INSERT INTO posts(slug,title,content_md,excerpt,status,post_type,published_at,category_id)
         VALUES (?,?,?,?,?,?,?,?)",
    )
    .bind(&slug)
    .bind(&input.title)
    .bind(&input.content_md)
    .bind(&excerpt)
    .bind(status)
    .bind(post_type)
    .bind(published_at.map(|d| d.to_rfc3339_opts(SecondsFormat::Nanos, true)))
    .bind(input.category_id)
    .execute(db)
    .await?
    .last_insert_rowid();
    if !input.tags.is_empty() {
        set_post_tags(db, id, &input.tags).await?;
    }
    get_post(db, id)
        .await?
        .ok_or_else(|| AppError::Internal("建文后读取失败".into()))
}

pub async fn get_post(db: &Db, id: i64) -> Result<Option<Post>, AppError> {
    let sql = format!("SELECT {POST_COLUMNS} FROM posts WHERE id = ?");
    let row = sqlx::query_as::<_, PostRow>(&sql)
        .bind(id)
        .fetch_optional(db)
        .await?;
    Ok(row.map(Post::from))
}

pub async fn get_post_by_slug(db: &Db, slug: &str) -> Result<Option<Post>, AppError> {
    let sql = format!("SELECT {POST_COLUMNS} FROM posts WHERE slug = ?");
    let row = sqlx::query_as::<_, PostRow>(&sql)
        .bind(slug)
        .fetch_optional(db)
        .await?;
    Ok(row.map(Post::from))
}

/// 依据 opts 拼接 WHERE 子句（全部参数化，`?` 按 status/post_type/category/tag 顺序）。
fn build_list_where(opts: &PostListOptions) -> String {
    let mut sql = String::from(" WHERE 1=1");
    if opts.status.is_some() {
        sql.push_str(" AND status = ?");
    }
    if opts.post_type.is_some() {
        sql.push_str(" AND post_type = ?");
    }
    if opts.category_slug.is_some() {
        sql.push_str(
            " AND EXISTS (SELECT 1 FROM categories c WHERE c.id = posts.category_id AND c.slug = ?)",
        );
    }
    if opts.tag_slug.is_some() {
        sql.push_str(
            " AND EXISTS (SELECT 1 FROM post_tags pt JOIN tags t ON t.id = pt.tag_id \
             WHERE pt.post_id = posts.id AND t.slug = ?)",
        );
    }
    sql
}

pub async fn list_posts(db: &Db, opts: PostListOptions) -> Result<(Vec<Post>, i64), AppError> {
    let where_sql = build_list_where(&opts);
    let count_sql = format!("SELECT COUNT(*) FROM posts{where_sql}");
    let mut count_q = sqlx::query(&count_sql);
    if let Some(status) = opts.status {
        count_q = count_q.bind(status.to_str());
    }
    if let Some(post_type) = opts.post_type {
        count_q = count_q.bind(post_type.to_str());
    }
    if let Some(cat) = opts.category_slug.as_deref() {
        count_q = count_q.bind(cat);
    }
    if let Some(tag) = opts.tag_slug.as_deref() {
        count_q = count_q.bind(tag);
    }
    let total: i64 = count_q.fetch_one(db).await?.get(0);

    let item_sql = format!(
        "SELECT {POST_COLUMNS} FROM posts{where_sql} ORDER BY published_at DESC, id DESC LIMIT ? OFFSET ?"
    );
    let mut q = sqlx::query_as::<_, PostRow>(&item_sql);
    if let Some(status) = opts.status {
        q = q.bind(status.to_str());
    }
    if let Some(post_type) = opts.post_type {
        q = q.bind(post_type.to_str());
    }
    if let Some(cat) = opts.category_slug.as_deref() {
        q = q.bind(cat);
    }
    if let Some(tag) = opts.tag_slug.as_deref() {
        q = q.bind(tag);
    }
    q = q.bind(opts.page_size).bind((opts.page - 1) * opts.page_size);
    let rows = q.fetch_all(db).await?;
    let items: Vec<Post> = rows.into_iter().map(Post::from).collect();
    Ok((items, total))
}

pub async fn update_post(db: &Db, id: i64, input: UpdatePost) -> Result<Post, AppError> {
    let old = get_post(db, id)
        .await?
        .ok_or_else(|| AppError::NotFound("文章不存在".into()))?;
    let mut sets: Vec<&str> = Vec::new();
    let mut values: Vec<BindVal> = Vec::new();

    if let Some(title) = input.title {
        sets.push("title = ?");
        values.push(BindVal::Text(title));
    }
    if let Some(content_md) = input.content_md {
        sets.push("content_md = ?");
        values.push(BindVal::Text(content_md));
    }
    if let Some(excerpt) = input.excerpt {
        sets.push("excerpt = ?");
        values.push(BindVal::Text(excerpt));
    }
    if let Some(slug) = input.slug {
        // 空串视为不变，与 create_post 的过滤行为对齐
        if !slug.trim().is_empty() {
            let slug = slugify(&slug).await;
            sets.push("slug = ?");
            values.push(BindVal::Text(slug));
        }
    }
    if let Some(status) = input.status {
        sets.push("status = ?");
        values.push(BindVal::Text(status.to_str().to_string()));
        // 草稿 → 发布 时设置发布时间
        if old.status == PostStatus::Draft && status == PostStatus::Published {
            sets.push("published_at = ?");
            values.push(BindVal::Text(
                Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true),
            ));
        }
    }
    if let Some(post_type) = input.post_type {
        sets.push("post_type = ?");
        values.push(BindVal::Text(post_type.to_str().to_string()));
    }
    if let Some(category_id) = input.category_id {
        sets.push("category_id = ?");
        values.push(BindVal::Int(category_id));
    }
    sets.push("updated_at = ?");
    values.push(BindVal::Text(Utc::now().to_rfc3339()));

    let mut sql = String::from("UPDATE posts SET ");
    sql.push_str(&sets.join(", "));
    sql.push_str(" WHERE id = ?");
    values.push(BindVal::Int(id));

    let mut q = sqlx::query(&sql);
    for v in values {
        match v {
            BindVal::Text(s) => q = q.bind(s),
            BindVal::Int(i) => q = q.bind(i),
        }
    }
    q.execute(db).await?;
    if let Some(tags) = input.tags {
        set_post_tags(db, id, &tags).await?;
    }
    get_post(db, id)
        .await?
        .ok_or_else(|| AppError::Internal("更新后读取失败".into()))
}

pub async fn delete_post(db: &Db, id: i64) -> Result<(), AppError> {
    let r = sqlx::query("DELETE FROM posts WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound("文章不存在".into()));
    }
    Ok(())
}

pub async fn increment_views(db: &Db, id: i64) -> Result<(), AppError> {
    sqlx::query("UPDATE posts SET views = views + 1 WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn adjacent_posts(db: &Db, p: &Post) -> Result<(Option<Post>, Option<Post>), AppError> {
    // 只关联普通文章：独立页（page）走 /page/ 路由，不能作为文章上一篇/下一篇
    let prev = match &p.published_at {
        Some(ts) => {
            let sql = format!(
                "SELECT {POST_COLUMNS} FROM posts \
                 WHERE published_at < ? AND post_type = 'post' \
                 ORDER BY published_at DESC, id DESC LIMIT 1"
            );
            let row = sqlx::query_as::<_, PostRow>(&sql)
                .bind(ts.to_rfc3339_opts(SecondsFormat::Nanos, true))
                .fetch_optional(db)
                .await?;
            row.map(Post::from)
        }
        None => None,
    };
    let next = match &p.published_at {
        Some(ts) => {
            let sql = format!(
                "SELECT {POST_COLUMNS} FROM posts \
                 WHERE published_at > ? AND post_type = 'post' \
                 ORDER BY published_at ASC, id ASC LIMIT 1"
            );
            let row = sqlx::query_as::<_, PostRow>(&sql)
                .bind(ts.to_rfc3339_opts(SecondsFormat::Nanos, true))
                .fetch_optional(db)
                .await?;
            row.map(Post::from)
        }
        None => None,
    };
    Ok((prev, next))
}

pub async fn count_posts(db: &Db) -> Result<i64, AppError> {
    let total: i64 = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM posts")
        .fetch_one(db)
        .await?;
    Ok(total)
}

pub async fn list_tags_of_post(db: &Db, post_id: i64) -> Result<Vec<Tag>, AppError> {
    let rows = sqlx::query_as::<_, Tag>(
        "SELECT t.id, t.slug, t.name FROM tags t \
         JOIN post_tags pt ON pt.tag_id = t.id WHERE pt.post_id = ? ORDER BY t.id",
    )
    .bind(post_id)
    .fetch_all(db)
    .await?;
    Ok(rows)
}

pub async fn set_post_tags(db: &Db, post_id: i64, tags: &[String]) -> Result<(), AppError> {
    sqlx::query("DELETE FROM post_tags WHERE post_id = ?")
        .bind(post_id)
        .execute(db)
        .await?;
    for tag in tags {
        let t = crate::services::taxonomy::ensure_tag(db, tag).await?;
        sqlx::query("INSERT OR IGNORE INTO post_tags(post_id, tag_id) VALUES (?, ?)")
            .bind(post_id)
            .bind(t.id)
            .execute(db)
            .await?;
    }
    Ok(())
}

/// 纯函数：去掉 markdown 标记后取前 150 字符。
pub async fn excerpt_of(md: &str) -> String {
    let mut text = String::new();
    for line in md.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("```") || line.starts_with("~~~") {
            continue;
        }
        let line = line.trim_start_matches('#').trim();
        if line.is_empty() {
            continue;
        }
        text.push_str(line);
        text.push(' ');
        if text.chars().count() >= 150 {
            break;
        }
    }
    strip_md_markers(&text)
        .chars()
        .take(150)
        .collect::<String>()
        .trim()
        .to_string()
}

/// 去掉 markdown 图片/链接语法，删除行内标记字符。
fn strip_md_markers(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let is_image = chars[i] == '!' && i + 1 < chars.len() && chars[i + 1] == '[';
        if is_image || chars[i] == '[' {
            let open = if is_image { i + 2 } else { i + 1 };
            if let Some(close) = chars[open..].iter().position(|&c| c == ']') {
                let after = open + close;
                if chars.get(after + 1) == Some(&'(') {
                    if let Some(end) = chars[after + 2..].iter().position(|&c| c == ')') {
                        if !is_image {
                            out.extend(chars[i + 1..after].iter());
                        }
                        i = after + 2 + end + 1;
                        continue;
                    }
                }
            }
        }
        if !matches!(chars[i], '*' | '_' | '`' | '~') {
            out.push(chars[i]);
        }
        i += 1;
    }
    out
}
