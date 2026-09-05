//! 文章服务：slug 生成、文章 CRUD、列表分页、相邻文章与标签管理。

use crate::db::Db;
use crate::error::AppError;
use crate::models::{Post, PostStatus, PostType, Tag};
use chrono::{DateTime, SecondsFormat, Utc};
use sqlx::FromRow;
use sqlx::Row;
use std::str::FromStr;
use uuid::Uuid;

/// 动态 SQL 的绑定值（`SqliteArgumentValue` 在 sqlx 0.8.6 不支持 `Type`，故用本地枚举）。
enum BindVal {
    Text(String),
    Int(i64),
    Null,
}

pub struct NewPost {
    pub title: String,
    pub content_md: String,
    pub excerpt: Option<String>,
    pub slug: Option<String>,
    pub status: PostStatus,
    pub post_type: PostType,
    pub category_id: Option<i64>,
    pub column_id: Option<i64>,
    /// 标签名列表
    pub tags: Vec<String>,
}

pub struct UpdatePost {
    /// None = 不变；slug/tags 特殊：Some(_) 即替换；
    /// excerpt/category_id/column_id 为 `Option<Option<T>>`：Some(Some(v)) = 设值、Some(None) = 显式清空、None = 不变
    pub title: Option<String>,
    pub content_md: Option<String>,
    pub excerpt: Option<Option<String>>,
    pub slug: Option<String>,
    pub status: Option<PostStatus>,
    pub post_type: Option<PostType>,
    pub category_id: Option<Option<i64>>,
    pub column_id: Option<Option<i64>>,
    pub tags: Option<Vec<String>>,
}

pub struct PostListOptions {
    pub status: Option<PostStatus>,
    /// None = 全部类型（向后兼容，T3 调用不受影响）
    pub post_type: Option<PostType>,
    pub category_slug: Option<String>,
    pub tag_slug: Option<String>,
    /// 专栏筛选："slug"（按 posts.column_id 关联 columns.slug）
    pub column_slug: Option<String>,
    /// 月份筛选："YYYY-MM"（按 published_at 前缀）
    pub month: Option<String>,
    /// 排序（字段白名单）；None = 默认时间倒序
    pub sort: Option<PostSort>,
    pub page: i64,
    pub page_size: i64,
}

/// 列表排序：字段名（白名单）+ 方向。见 `order_by_clause` 映射，防注入。
#[derive(Clone, Copy)]
pub struct PostSort {
    pub field: &'static str,
    pub asc: bool,
}

/// 列表 ORDER BY 子句：排序字段白名单映射，非法字段回退默认时间倒序。
pub(crate) fn order_by_clause(sort: Option<PostSort>) -> String {
    let Some(s) = sort else {
        return "ORDER BY published_at DESC, id DESC".to_string();
    };
    let field = match s.field {
        "title" => "title",
        "views" => "views",
        "like_count" => "like_count",
        "updated_at" => "updated_at",
        "published_at" => "published_at",
        "created_at" => "created_at",
        "status" => "status",
        "column_sort" => "column_sort",
        _ => return "ORDER BY published_at DESC, id DESC".to_string(),
    };
    let dir = if s.asc { "ASC" } else { "DESC" };
    // 专栏内文章按自定义顺序（column_sort）：同序回退按 id 升序（加入顺序）
    let tail = if s.field == "column_sort" { "id ASC" } else { "id DESC" };
    format!("ORDER BY {field} {dir}, {tail}")
}

/// 数据库行结构：枚举字段以 String 存取，经 `to_str`/`from_str` 与模型互转。
/// `pub(crate)`：后台文章管理（`admin/posts.rs` 关键词列表查询）复用。
#[derive(FromRow)]
pub(crate) struct PostRow {
    id: i64,
    uuid: String,
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
    like_count: i64,
    category_id: Option<i64>,
    column_id: Option<i64>,
    column_sort: i64,
}

impl From<PostRow> for Post {
    fn from(r: PostRow) -> Self {
        Post {
            id: r.id,
            uuid: r.uuid,
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
            like_count: r.like_count,
            category_id: r.category_id,
            column_id: r.column_id,
            column_sort: r.column_sort,
        }
    }
}

/// `pub(crate)`：后台文章管理复用。
pub(crate) const POST_COLUMNS: &str = "id, uuid, slug, title, content_md, excerpt, status, post_type, \
    published_at, created_at, updated_at, views, like_count, category_id, column_id, column_sort";

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
    // slug 来源：显式 slug（trim 非空）→ 标题；slugify 结果为空（纯标点如
    // `---`）时回退标题再 slugify，避免写入空 slug（I4）。
    let slug_source = input
        .slug
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| input.title.clone());
    let mut slug_candidate = slugify(&slug_source).await;
    if slug_candidate.is_empty() {
        slug_candidate = slugify(&input.title).await;
    }
    let slug = unique_slug(db, &slug_candidate).await?;
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
    let uuid = Uuid::new_v4().to_string();
    let id = sqlx::query(
        "INSERT INTO posts(uuid,slug,title,content_md,excerpt,status,post_type,published_at,category_id,column_id)
         VALUES (?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(&uuid)
    .bind(&slug)
    .bind(&input.title)
    .bind(&input.content_md)
    .bind(&excerpt)
    .bind(status)
    .bind(post_type)
    .bind(published_at.map(|d| d.to_rfc3339_opts(SecondsFormat::Nanos, true)))
    .bind(input.category_id)
    .bind(input.column_id)
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

pub async fn get_post_by_uuid(db: &Db, uuid: &str) -> Result<Option<Post>, AppError> {
    let sql = format!("SELECT {POST_COLUMNS} FROM posts WHERE uuid = ?");
    let row = sqlx::query_as::<_, PostRow>(&sql)
        .bind(uuid)
        .fetch_optional(db)
        .await?;
    Ok(row.map(Post::from))
}

pub fn public_post_path(post: &Post) -> String {
    match post.post_type {
        PostType::Post => format!("/post/{}", post.uuid),
        PostType::Page => format!("/page/{}", post.slug),
    }
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
    if opts.column_slug.is_some() {
        sql.push_str(
            " AND EXISTS (SELECT 1 FROM columns c WHERE c.id = posts.column_id AND c.slug = ?)",
        );
    }
    if opts.month.is_some() {
        sql.push_str(" AND substr(published_at, 1, 7) = ?");
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
    if let Some(month) = opts.month.as_deref() {
        count_q = count_q.bind(month);
    }
    if let Some(column) = opts.column_slug.as_deref() {
        count_q = count_q.bind(column);
    }
    let total: i64 = count_q.fetch_one(db).await?.get(0);

    let item_sql = format!(
        "SELECT {POST_COLUMNS} FROM posts{where_sql} {} LIMIT ? OFFSET ?",
        order_by_clause(opts.sort)
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
    if let Some(month) = opts.month.as_deref() {
        q = q.bind(month);
    }
    if let Some(column) = opts.column_slug.as_deref() {
        q = q.bind(column);
    }
    q = q.bind(opts.page_size).bind((opts.page - 1) * opts.page_size);
    let rows = q.fetch_all(db).await?;
    let items: Vec<Post> = rows.into_iter().map(Post::from).collect();
    Ok((items, total))
}

/// 专栏内文章拖拽排序：按传入 post id 顺序重写 column_sort（0..n）。
pub async fn reorder_column_posts(db: &Db, ids: &[i64]) -> Result<(), AppError> {
    for (idx, id) in ids.iter().enumerate() {
        sqlx::query("UPDATE posts SET column_sort = ? WHERE id = ?")
            .bind(idx as i64)
            .bind(id)
            .execute(db)
            .await?;
    }
    Ok(())
}

/// 搜索命中：文章 + FTS5 高亮片段（`<mark>` 包裹，空则回退摘要）。
pub struct SearchHit {
    pub post: Post,
    pub snippet: String,
}

/// FTS5 全文搜索：按相关性排序分页返回命中与总数。
///
/// 用户输入整体作为短语查询：双引号翻倍（`"` → `""`）后包在双引号里，
/// 使 `--`、`'` 等 FTS5 语法字符全部字面化；解析不了查询时按空结果处理，
/// 不把 500 抛给搜索页。`snippet()` 列索引 0=title、1=content_md。
pub async fn search_posts(
    db: &Db,
    q: &str,
    page: i64,
    page_size: i64,
) -> Result<(Vec<SearchHit>, i64), AppError> {
    let q = q.trim();
    if q.is_empty() {
        return Ok((vec![], 0));
    }
    let escaped = q.replace('"', "\"\"");
    let match_expr = format!("\"{escaped}\"");
    let offset = (page - 1).max(0) * page_size;

    // 不 SELECT rank：FTS5 的 rank 是 REAL，sqlx 0.8.6 严格类型检查下无法解码为 i64；
    // `ORDER BY rank` 无需选中该列。JOIN posts 只放行已发布普通文章：
    // posts_fts 触发器无条件索引全部行（含草稿与独立页），必须在此过滤。
    let rows: Vec<i64> = match sqlx::query_scalar::<_, i64>(
        "SELECT p.id FROM posts_fts f JOIN posts p ON p.id = f.rowid \
         WHERE posts_fts MATCH ? AND p.status = 'published' AND p.post_type = 'post' \
         ORDER BY rank LIMIT ? OFFSET ?",
    )
    .bind(&match_expr)
    .bind(page_size)
    .bind(offset)
    .fetch_all(db)
    .await
    {
        Ok(rows) => rows,
        Err(e) if fts_syntax_error(&e) => return Ok((vec![], 0)),
        Err(e) => return Err(e.into()),
    };

    let mut hits = Vec::with_capacity(rows.len());
    for id in rows {
        let post = get_post(db, id)
            .await?
            .ok_or_else(|| AppError::Internal("FTS 命中丢失".into()))?;
        let snippet = hit_snippet(db, id, &match_expr, &post.excerpt).await?;
        hits.push(SearchHit { post, snippet });
    }

    let total: i64 = match sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM posts_fts f JOIN posts p ON p.id = f.rowid \
         WHERE posts_fts MATCH ? AND p.status = 'published' AND p.post_type = 'post'",
    )
    .bind(&match_expr)
    .fetch_one(db)
    .await
    {
        Ok(n) => n,
        Err(e) if fts_syntax_error(&e) => 0,
        Err(e) => return Err(e.into()),
    };
    Ok((hits, total))
}

/// 命中行的高亮片段：正文命中优先展示正文片段，其次标题片段，均无则回退摘要。
/// `snippet()` 对未命中的列返回无高亮的原文，据此判断命中列。
///
/// 安全：snippet() 输出正文原文，正文里的 HTML（如 `<script>`）在文章页经
/// pulldown-cmark 转义，但搜索页片段直接 `| safe` 输出——故先让 snippet() 用
/// 哨兵字符包裹命中词，整体 `html_escape` 后再还原 `<mark>`，杜绝存储型 XSS（C1）。
async fn hit_snippet(
    db: &Db,
    id: i64,
    match_expr: &str,
    excerpt: &str,
) -> Result<String, AppError> {
    let sql = format!(
        "SELECT snippet(posts_fts, 0, '{SNIPPET_MARK_OPEN}', '{SNIPPET_MARK_CLOSE}', '…', 12), \
                snippet(posts_fts, 1, '{SNIPPET_MARK_OPEN}', '{SNIPPET_MARK_CLOSE}', '…', 12) \
         FROM posts_fts WHERE rowid = ? AND posts_fts MATCH ?",
    );
    let (title_snip, content_snip): (String, String) =
        sqlx::query_as::<_, (String, String)>(&sql)
            .bind(id)
            .bind(match_expr)
            .fetch_one(db)
            .await?;
    let snippet = if content_snip.contains(SNIPPET_MARK_OPEN) {
        content_snip
    } else if title_snip.contains(SNIPPET_MARK_OPEN) {
        title_snip
    } else {
        excerpt.to_string()
    };
    // 正文/标题/摘要原文一律先 HTML 转义，再把哨兵还原为 `<mark>` 高亮
    Ok(crate::util::html_escape(&snippet)
        .replace(SNIPPET_MARK_OPEN, "<mark>")
        .replace(SNIPPET_MARK_CLOSE, "</mark>"))
}

/// FTS5 snippet() 高亮哨兵：用正文几乎不可能出现的控制字符包裹命中词，
/// 转义后还原，避免把正文里的原始 HTML 原样带回（见 `hit_snippet` 注释）。
const SNIPPET_MARK_OPEN: &str = "\u{1}";
const SNIPPET_MARK_CLOSE: &str = "\u{2}";

/// FTS5 对无法解析的查询报 `fts5: syntax error ...`，视为无结果而非 500。
fn fts_syntax_error(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(dbe) if dbe.message().contains("syntax error"))
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
        // Some(None) = 显式清空摘要（空串）
        values.push(BindVal::Text(excerpt.unwrap_or_default()));
    }
    if let Some(slug) = input.slug {
        // slugify 后为空（纯标点如 `---`）视为不变，避免写入空 slug（I4）
        let slug = slugify(&slug).await;
        if !slug.is_empty() {
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
        // Some(None) = 显式清空分类（SET NULL）
        match category_id {
            Some(id) => values.push(BindVal::Int(id)),
            None => values.push(BindVal::Null),
        }
    }
    if let Some(column_id) = input.column_id {
        sets.push("column_id = ?");
        // Some(None) = 显式清空专栏（SET NULL）
        match column_id {
            Some(id) => values.push(BindVal::Int(id)),
            None => values.push(BindVal::Null),
        }
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
            BindVal::Null => q = q.bind(None::<i64>),
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

/// 发布热力图：近 `days` 天按天统计已发布文章数（UTC 日期前缀）。
/// 返回 `Vec<(日期 YYYY-MM-DD, 数量)>`，仅含有发布的日期。
pub async fn heatmap(db: &Db, days: i64) -> Result<Vec<(String, i64)>, AppError> {
    let since = chrono::Utc::now() - chrono::Duration::days(days);
    let since = since.format("%Y-%m-%dT00:00:00Z").to_string();
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT substr(published_at, 1, 10) AS d, COUNT(*) AS c
         FROM posts
         WHERE status = 'published' AND post_type = 'post'
           AND published_at IS NOT NULL AND published_at >= ?
         GROUP BY d",
    )
    .bind(&since)
    .fetch_all(db)
    .await?;
    Ok(rows)
}

/// 发布日历：近 `days` 天按天聚合发布动态（文章发布 + 说说，不含文章更新）。
/// 每项 `(日期, 动态条数, Vec<(类型 post|moment, 标题)>)`。
pub async fn activity_calendar(
    db: &Db,
    days: i64,
) -> Result<Vec<(String, i64, Vec<(String, String)>)>, AppError> {
    use std::collections::HashMap;
    let since = chrono::Utc::now() - chrono::Duration::days(days);
    let since = since.format("%Y-%m-%dT00:00:00Z").to_string();

    let posts: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT substr(published_at, 1, 10) AS d, title, 'post' AS kind
         FROM posts
         WHERE status = 'published' AND post_type = 'post'
           AND published_at IS NOT NULL AND published_at >= ?",
    )
    .bind(&since)
    .fetch_all(db)
    .await?;
    let moments: Vec<(String, String)> = sqlx::query_as(
        "SELECT substr(created_at, 1, 10) AS d, content FROM moments
         WHERE created_at >= ?",
    )
    .bind(&since)
    .fetch_all(db)
    .await?;

    let mut map: HashMap<String, (i64, Vec<(String, String)>)> = HashMap::new();
    for (d, title, kind) in posts {
        let e = map.entry(d).or_insert_with(|| (0, Vec::new()));
        // 同日同文去重
        if !e.1.iter().any(|(k, t)| *k == kind && *t == title) {
            e.0 += 1;
            e.1.push((kind, title));
        }
    }
    for (d, content) in moments {
        let title = content
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(30)
            .collect::<String>();
        let e = map.entry(d).or_insert_with(|| (0, Vec::new()));
        e.0 += 1;
        e.1.push(("moment".to_string(), title));
    }
    Ok(map
        .into_iter()
        .map(|(d, (c, items))| (d, c, items))
        .collect())
}

/// 一条活动（文章发布 / 说说）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct Activity {
    pub kind: &'static str, // "post" | "moment"
    pub title: String,      // 文章标题或说说首段
    pub url: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// 最近活动流：合并已发布文章与说说，按时间倒序。
/// `days` 为回溯窗口，`limit` 为每类最大条数。
pub async fn recent_activity(
    db: &Db,
    days: i64,
    limit: i64,
) -> Result<Vec<Activity>, AppError> {
    let since = chrono::Utc::now() - chrono::Duration::days(days);
    let since = since.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let posts: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT published_at, title, slug FROM posts
         WHERE status = 'published' AND post_type = 'post'
           AND published_at IS NOT NULL AND published_at >= ?
         ORDER BY published_at DESC LIMIT ?",
    )
    .bind(&since)
    .bind(limit)
    .fetch_all(db)
    .await?;
    let moments: Vec<(String, String)> = sqlx::query_as(
        "SELECT created_at, content FROM moments
         WHERE created_at >= ? ORDER BY created_at DESC LIMIT ?",
    )
    .bind(&since)
    .bind(limit)
    .fetch_all(db)
    .await?;

    let mut acts: Vec<Activity> = Vec::with_capacity(posts.len() + moments.len());
    for (ts, title, slug) in posts {
        acts.push(Activity {
            kind: "post",
            title,
            url: Some(format!("/post/{slug}")),
            created_at: parse_ts(&ts),
        });
    }
    for (ts, content) in moments {
        let title = content
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(60)
            .collect::<String>();
        acts.push(Activity {
            kind: "moment",
            title,
            url: Some("/moments".into()),
            created_at: parse_ts(&ts),
        });
    }
    acts.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(acts)
}

/// 解析 RFC3339 时间串；失败回退 Unix 纪元（不应发生）。
fn parse_ts(s: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::DateTime::<chrono::Utc>::UNIX_EPOCH)
}

/// 月份归档列表：已发布文章按 `YYYY-MM` 去重倒序。
pub async fn month_list(db: &Db) -> Result<Vec<String>, AppError> {
    let months: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT substr(published_at, 1, 7) AS m FROM posts
         WHERE status = 'published' AND post_type = 'post'
           AND published_at IS NOT NULL
         ORDER BY m DESC",
    )
    .fetch_all(db)
    .await?;
    Ok(months)
}

/// 指定分类/标签下已发布文章的月份列表（去重倒序），供归档/标签页侧栏筛选。
/// 两者均为 None 时与 `month_list` 等价。
pub async fn month_list_filtered(
    db: &Db,
    category_slug: Option<&str>,
    tag_slug: Option<&str>,
) -> Result<Vec<String>, AppError> {
    let sql = if tag_slug.is_some() {
        "SELECT DISTINCT substr(p.published_at, 1, 7) AS m
         FROM posts p
         JOIN post_tags pt ON pt.post_id = p.id
         JOIN tags t ON t.id = pt.tag_id
         WHERE p.status = 'published' AND p.post_type = 'post'
           AND p.published_at IS NOT NULL AND t.slug = ?
         ORDER BY m DESC"
            .to_string()
    } else if category_slug.is_some() {
        "SELECT DISTINCT substr(p.published_at, 1, 7) AS m
         FROM posts p
         JOIN categories c ON c.id = p.category_id
         WHERE p.status = 'published' AND p.post_type = 'post'
           AND p.published_at IS NOT NULL AND c.slug = ?
         ORDER BY m DESC"
            .to_string()
    } else {
        return month_list(db).await;
    };
    let mut q = sqlx::query_scalar::<_, String>(&sql);
    if let Some(tag) = tag_slug {
        q = q.bind(tag);
    } else if let Some(cat) = category_slug {
        q = q.bind(cat);
    }
    let months: Vec<String> = q.fetch_all(db).await?;
    Ok(months)
}
