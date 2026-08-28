//! 后台文章管理：列表、新建/编辑、更新、删除与 Vditor 自动保存。
//!
//! 鉴权约定：GET 页面未登录 302 跳登录；POST 一律先 `require_admin`（未登录
//! 401 JSON）再过 CSRF（表单字段 `csrf`，autosave 走 `X-CSRF-Token` 头）。
//! 列表排序沿用 `posts::list_posts` 的 `published_at DESC, id DESC`。

use crate::db::Db;
use crate::error::AppError;
use crate::models::{Category, Post, PostStatus, PostType, Tag};
use crate::services::{posts, taxonomy};
use crate::{session, AppState};
use axum::extract::{Form, OriginalUri, Path, Query, State};
use axum::http::HeaderMap;
use axum::response::Response;
use axum::Json;
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use std::collections::HashMap;
use tower_sessions::Session;

/// 列表每页条数。
const PAGE_SIZE: i64 = 20;

// ---------- 列表 ----------

/// 生成固定链接：8 位短 uuid（唯一性由 `create_post` 的 unique_slug 兜底）。
fn short_slug() -> String {
    let full = uuid::Uuid::new_v4().simple().to_string();
    full[..8].to_string()
}

/// 从查询参数解析文章列表排序：`sort=字段` + `dir=asc|desc`；字段白名单校验，
/// 非法字段回退默认。无 sort 参数时默认按更新时间倒序（updated_at desc）——
/// 若沿用 `order_by_clause(None)` 的 published_at DESC，草稿（published_at 为 NULL）
/// 会被 SQLite 排到最后一页，「全部状态」下看起来就像没有草稿文章。
fn admin_sort(query: &HashMap<String, String>) -> Option<posts::PostSort> {
    let field: &'static str = match query.get("sort").map(String::as_str).unwrap_or("") {
        "title" => "title",
        "views" => "views",
        "like_count" => "like_count",
        "updated_at" => "updated_at",
        "published_at" => "published_at",
        "status" => "status",
        _ => return Some(posts::PostSort { field: "updated_at", asc: false }),
    };
    let asc = query.get("dir").map(String::as_str).unwrap_or("desc") == "asc";
    Some(posts::PostSort { field, asc })
}

pub async fn list(
    State(state): State<AppState>,
    session: Session,
    Query(query): Query<HashMap<String, String>>,
    uri: OriginalUri,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect(&state.config.base_path, "/admin/login");
    }
    let status = match query.get("status").map(String::as_str).unwrap_or("") {
        "draft" => Some(PostStatus::Draft),
        "published" => Some(PostStatus::Published),
        _ => None,
    };
    let category_slug = query.get("category").filter(|s| !s.is_empty()).cloned();
    // 列表默认显示全部类型；type=post/page 时按类型筛选（与模板默认选中「全部类型」一致）
    let post_type = match query.get("type").map(String::as_str).unwrap_or("all") {
        "page" => Some(PostType::Page),
        "post" => Some(PostType::Post),
        _ => None, // all 或未知值：显示全部
    };
    let q = query
        .get("q")
        .map(String::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let page = query
        .get("page")
        .and_then(|p| p.parse::<i64>().ok())
        .filter(|&p| p > 0)
        .unwrap_or(1);

    // 无关键词走 `PostListOptions` 组合筛选；有关键词时 `PostListOptions`
    // 不含关键词，落到标题 LIKE 专用查询。
    let (items, total) = if q.is_empty() {
        match posts::list_posts(
            &state.db,
            posts::PostListOptions {
                status,
                post_type,
                category_slug: category_slug.clone(),
                tag_slug: None,
                column_slug: None,
                month: None,
                sort: admin_sort(&query),
                page,
                page_size: PAGE_SIZE,
            },
        )
        .await
        {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("后台文章列表查询失败: {e:?}");
                (vec![], 0)
            }
        }
    } else {
        match list_by_keyword(&state.db, status, post_type, category_slug.as_deref(), &q, admin_sort(&query), page).await {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("后台文章关键词查询失败: {e:?}");
                (vec![], 0)
            }
        }
    };

    let categories = taxonomy::list_categories(&state.db).await.unwrap_or_default();
    let cat_names: HashMap<i64, String> = categories
        .iter()
        .map(|c| (c.id, c.name.clone()))
        .collect();

    let (mut ctx, _csrf) = super::base_ctx(&state, &session, uri.path()).await;
    ctx.insert("posts", &post_list_value(&items, &cat_names));
    ctx.insert("total", &total);
    ctx.insert("page", &page);
    ctx.insert("total_pages", &((total + PAGE_SIZE - 1) / PAGE_SIZE).max(1));
    ctx.insert(
        "filters",
        &json!({
            "status": query.get("status").map(String::as_str).unwrap_or(""),
            "type": query.get("type").map(String::as_str).unwrap_or("all"),
            "category": category_slug.unwrap_or_default(),
            "q": q,
            "sort": query.get("sort").map(String::as_str).unwrap_or("updated_at"),
            "dir": query.get("dir").map(String::as_str).unwrap_or("desc"),
        }),
    );
    ctx.insert("categories", &categories_value(&categories));
    super::render_admin(&state, "posts_list.html", &ctx)
}

/// 列表行 JSON：category 显示名由 Rust 侧查表拼好，模板无需再按 id 索引。
fn post_list_value(items: &[Post], cat_names: &HashMap<i64, String>) -> Value {
    json!(items
        .iter()
        .map(|p| json!({
            "id": p.id,
            "title": p.title,
            "slug": p.slug,
            "status": p.status.to_str(),
            "post_type": p.post_type.to_str(),
            "views": p.views,
            "like_count": p.like_count,
            "category": p
                .category_id
                .and_then(|id| cat_names.get(&id))
                .cloned()
                .unwrap_or_default(),
            "updated_at": super::format_local(p.updated_at),
        }))
        .collect::<Vec<_>>())
}

/// 标题关键词 + 类型/状态/分类组合查询（排序与 `list_posts` 一致）。
async fn list_by_keyword(
    db: &Db,
    status: Option<PostStatus>,
    post_type: Option<PostType>,
    category_slug: Option<&str>,
    q: &str,
    sort: Option<posts::PostSort>,
    page: i64,
) -> Result<(Vec<Post>, i64), AppError> {
    let mut where_sql = String::from(" WHERE title LIKE ?");
    let mut binds: Vec<String> = vec![format!("%{q}%")];
    if let Some(st) = status {
        where_sql.push_str(" AND status = ?");
        binds.push(st.to_str().to_string());
    }
    if let Some(ty) = post_type {
        where_sql.push_str(" AND post_type = ?");
        binds.push(ty.to_str().to_string());
    }
    if let Some(cat) = category_slug {
        where_sql.push_str(
            " AND EXISTS (SELECT 1 FROM categories c WHERE c.id = posts.category_id AND c.slug = ?)",
        );
        binds.push(cat.to_string());
    }

    let count_sql = format!("SELECT COUNT(*) FROM posts{where_sql}");
    let mut count_q = sqlx::query(&count_sql);
    for b in &binds {
        count_q = count_q.bind(b);
    }
    let total: i64 = count_q.fetch_one(db).await?.get(0);

    let item_sql = format!(
        "SELECT {cols} FROM posts{where_sql} {order} LIMIT ? OFFSET ?",
        cols = posts::POST_COLUMNS,
        order = posts::order_by_clause(sort)
    );
    let mut q = sqlx::query_as::<_, posts::PostRow>(&item_sql);
    for b in &binds {
        q = q.bind(b);
    }
    q = q.bind(PAGE_SIZE).bind((page - 1) * PAGE_SIZE);
    let rows = q.fetch_all(db).await?;
    let items: Vec<Post> = rows.into_iter().map(Into::into).collect();
    Ok((items, total))
}

// ---------- 新建 / 编辑 ----------

pub async fn new_page(
    State(state): State<AppState>,
    session: Session,
    uri: OriginalUri,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect(&state.config.base_path, "/admin/login");
    }
    render_new(&state, &session, uri.path(), "").await
}

/// 新建页上下文（创建失败回显用）。
async fn render_new(state: &AppState, session: &Session, path: &str, error_tip: &str) -> Response {
    let categories = taxonomy::list_categories(&state.db).await.unwrap_or_default();
    let columns = crate::services::columns::list_columns(&state.db)
        .await
        .unwrap_or_default();
    let tags = taxonomy::list_tags(&state.db).await.unwrap_or_default();
    let (mut ctx, _csrf) = super::base_ctx(state, session, path).await;
    ctx.insert("post", &empty_post_value());
    ctx.insert("categories", &categories_value(&categories));
    ctx.insert("columns", &columns_value(&columns));
    ctx.insert("all_tags", &tags_value(&tags));
    ctx.insert("error_tip", error_tip);
    // 新建页正文模板：覆盖编辑器支持的全部 Markdown 标记与样式。
    // 注入 JSON 字符串字面量（json!(..).to_string() 带引号与转义），模板经 safe 原样输出为 JS 字符串。
    ctx.insert("new_post_template", &json!(NEW_POST_TEMPLATE).to_string());
    super::render_admin(state, "post_edit.html", &ctx)
}

/// 新建文章正文的 Markdown 语法模板（与编辑器 milkdown commonmark/gfm 支持范围对齐：
/// 标题/段落/加粗/斜体/行内代码/链接/引用/无序·有序列表/代码块/分割线/表格；
/// 严格遵循 Markdown 标准，不引入自定义标记；任务列表、删除线 milkdown 不支持，不列入模板）。
const NEW_POST_TEMPLATE: &str = "\
# 一级标题\n\
\n\
## 二级标题\n\
\n\
正文段落：支持 **加粗**、*斜体*、`行内代码`、[链接](https://example.com)。\n\
\n\
> 引用块\n\
\n\
- 无序列表项\n\
  - 嵌套子项\n\
\n\
1. 有序列表项\n\
2. 第二项\n\
\n\
```rust\n\
// 代码块：三个反引号 + 语言名 + 回车创建\n\
fn main() { println!(\"Hello\"); }\n\
```\n\
\n\
---\n\
\n\
图片：点击工具栏「插入图片」上传，或直接粘贴 / 拖拽到编辑区。
";

/// 新建页的空文章 JSON：id=0 时 `window._post` 无 id，自动保存不生效。
fn empty_post_value() -> Value {
    json!({
        "id": 0,
        "title": "",
        "content_md": "",
        "slug": "",
        "status": PostStatus::Draft.to_str(),
        "post_type": PostType::Post.to_str(),
        "excerpt": "",
        "tags": "",
        "category_id": 0,
        "column_id": 0,
    })
}

pub async fn edit_page(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<i64>,
    uri: OriginalUri,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect(&state.config.base_path, "/admin/login");
    }
    match render_edit(&state, &session, id, uri.path(), "", None).await {
        Ok(resp) => resp,
        Err(e) => {
            tracing::error!("渲染文章编辑页失败: {e:?}");
            super::redirect(&state.config.base_path, "/admin/posts")
        }
    }
}

/// 编辑页上下文（slug 冲突等错误回显用）。
///
/// `submitted`：可选的表单提交值。传入时用它回填表单（标题/正文/分类/标签/
/// slug/excerpt/status），保证校验失败回显不丢用户已填内容；None 时从 DB 取值。
async fn render_edit(
    state: &AppState,
    session: &Session,
    id: i64,
    path: &str,
    error_tip: &str,
    submitted: Option<&HashMap<String, String>>,
) -> Result<Response, AppError> {
    let Some(post) = posts::get_post(&state.db, id).await? else {
        return Ok(super::redirect(&state.config.base_path, "/admin/posts"));
    };
    let tags = posts::list_tags_of_post(&state.db, id)
        .await
        .unwrap_or_default();
    let categories = taxonomy::list_categories(&state.db).await.unwrap_or_default();
    let columns = crate::services::columns::list_columns(&state.db)
        .await
        .unwrap_or_default();
    let all_tags = taxonomy::list_tags(&state.db).await.unwrap_or_default();
    let (mut ctx, _csrf) = super::base_ctx(state, session, path).await;
    ctx.insert("post", &post_edit_value(&post, &tags, submitted));
    ctx.insert("categories", &categories_value(&categories));
    ctx.insert("columns", &columns_value(&columns));
    ctx.insert("all_tags", &tags_value(&all_tags));
    ctx.insert("error_tip", error_tip);
    Ok(super::render_admin(state, "post_edit.html", &ctx))
}

/// 标签 JSON：`[{name}]`，供标签 chips 下拉建议。
fn tags_value(tags: &[crate::models::Tag]) -> Value {
    json!(tags
        .iter()
        .map(|t| json!({ "name": t.name }))
        .collect::<Vec<_>>())
}

/// 编辑页文章 JSON：content_md 由模板 `{{ post.content_md }}` 进 `data-content`
/// 属性（tera autoescape 保证属性值安全），tags 拼成逗号分隔字符串回填输入框。
/// `submitted` 存在时逐字段覆盖为提交值（正文即使未改动也用提交值，保证不丢）。
fn post_edit_value(
    p: &Post,
    tags: &[Tag],
    submitted: Option<&HashMap<String, String>>,
) -> Value {
    let mut v = json!({
        "id": p.id,
        "title": p.title,
        "content_md": p.content_md,
        "slug": p.slug,
        "status": p.status.to_str(),
        "post_type": p.post_type.to_str(),
        "excerpt": p.excerpt,
        "tags": tags
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        "category_id": p.category_id.unwrap_or(0),
        "column_id": p.column_id.unwrap_or(0),
    });
    if let Some(f) = submitted {
        if let Some(t) = f.get("title") {
            v["title"] = json!(t);
        }
        if let Some(t) = f.get("content_md") {
            v["content_md"] = json!(t);
        }
        if let Some(t) = f.get("slug") {
            v["slug"] = json!(t);
        }
        if let Some(t) = f.get("status") {
            v["status"] = json!(t);
        }
        if let Some(t) = f.get("post_type") {
            v["post_type"] = json!(t);
        }
        if let Some(t) = f.get("excerpt") {
            v["excerpt"] = json!(t);
        }
        if let Some(t) = f.get("tags") {
            v["tags"] = json!(t);
        }
        if let Some(t) = f.get("category_id") {
            if let Ok(id) = t.trim().parse::<i64>() {
                v["category_id"] = json!(id);
            }
        }
        if let Some(t) = f.get("column_id") {
            if let Ok(id) = t.trim().parse::<i64>() {
                v["column_id"] = json!(id);
            }
        }
    }
    v
}

// ---------- 创建 ----------

pub async fn create(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<HashMap<String, String>>,
) -> Result<Response, AppError> {
    session::require_admin(&session).await?;
    session::verify_csrf(&session, form.get("csrf").map(String::as_str)).await?;
    let title = form.get("title").cloned().unwrap_or_default();
    if title.trim().is_empty() {
        return Ok(render_new(&state, &session, "/admin/posts/new", "标题不能为空").await);
    }
    let input = posts::NewPost {
        title,
        content_md: form.get("content_md").cloned().unwrap_or_default(),
        excerpt: optional_field(form.get("excerpt")),
        // 固定链接由系统自动生成（8 位短 uuid，unique_slug 兜底冲突）
        slug: Some(short_slug()),
        status: parse_status(form.get("status").map(String::as_str).unwrap_or("")),
        post_type: parse_post_type(form.get("post_type").map(String::as_str).unwrap_or("")),
        category_id: parse_id(form.get("category_id")),
        column_id: parse_id(form.get("column_id")),
        tags: parse_tags(form.get("tags")),
    };
    // 发布后自动返回列表；存草稿留在编辑页继续编辑
    let is_published = input.status == PostStatus::Published;
    match posts::create_post(&state.db, input).await {
        Ok(p) => {
            let loc = if is_published {
                "/admin/posts".to_string()
            } else {
                format!("/admin/posts/{}/edit", p.id)
            };
            Ok(super::redirect(&state.config.base_path, &loc))
        }
        Err(e) => {
            tracing::error!("创建文章失败: {e:?}");
            Ok(render_new(&state, &session, "/admin/posts/new", "保存失败，请重试").await)
        }
    }
}

// ---------- 更新 ----------

pub async fn update(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<i64>,
    Form(form): Form<HashMap<String, String>>,
) -> Result<Response, AppError> {
    session::require_admin(&session).await?;
    session::verify_csrf(&session, form.get("csrf").map(String::as_str)).await?;
    if posts::get_post(&state.db, id).await?.is_none() {
        return Ok(super::redirect(&state.config.base_path, "/admin/posts"));
    }
    let input = posts::UpdatePost {
        title: optional_field(form.get("title")),
        content_md: Some(form.get("content_md").cloned().unwrap_or_default()),
        // 编辑页已移除摘要输入：未提交（None）→ 保留原值；提交则设值/清空
        excerpt: form.get("excerpt").map(|v| optional_field(Some(v))),
        slug: None, // 固定链接由系统管理（uuid/创建时指定），编辑不再改动
        status: Some(parse_status(form.get("status").map(String::as_str).unwrap_or(""))),
        post_type: Some(parse_post_type(
            form.get("post_type").map(String::as_str).unwrap_or(""),
        )),
        // 空串 → Some(None) 显式清空分类；合法 id → Some(Some(id))；非法值忽略
        category_id: match form.get("category_id").map(String::as_str).unwrap_or("").trim() {
            "" => Some(None),
            _ => parse_id(form.get("category_id")).map(Some),
        },
        // 专栏同分类：空串清空、合法 id 设值
        column_id: match form.get("column_id").map(String::as_str).unwrap_or("").trim() {
            "" => Some(None),
            _ => parse_id(form.get("column_id")).map(Some),
        },
        tags: Some(parse_tags(form.get("tags"))),
    };
    // 发布后自动返回列表；存草稿留在编辑页继续编辑
    let is_published = input.status == Some(PostStatus::Published);
    let p = posts::update_post(&state.db, id, input).await?;
    let loc = if is_published {
        "/admin/posts".to_string()
    } else {
        format!("/admin/posts/{}/edit", p.id)
    };
    Ok(super::redirect(&state.config.base_path, &loc))
}

// ---------- 删除 ----------

pub async fn delete(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<i64>,
    Form(form): Form<HashMap<String, String>>,
) -> Result<Response, AppError> {
    session::require_admin(&session).await?;
    session::verify_csrf(&session, form.get("csrf").map(String::as_str)).await?;
    match posts::delete_post(&state.db, id).await {
        Ok(()) => Ok(super::redirect(&state.config.base_path, "/admin/posts")),
        Err(e) => {
            tracing::error!("删除文章失败: {e:?}");
            Ok(super::redirect(&state.config.base_path, "/admin/posts"))
        }
    }
}

// ---------- 自动保存 ----------

/// 自动保存请求体：仅正文。
#[derive(Deserialize)]
pub struct AutosaveForm {
    content_md: String,
}

/// 自动保存：Vditor 30s 轮询 / `pagehide` 触发，只更新正文，不改 status 等字段。
pub async fn autosave(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<i64>,
    headers: HeaderMap,
    Form(form): Form<AutosaveForm>,
) -> Result<Json<Value>, AppError> {
    session::require_admin(&session).await?;
    let token = headers.get("x-csrf-token").and_then(|v| v.to_str().ok());
    session::verify_csrf(&session, token).await?;
    if posts::get_post(&state.db, id).await?.is_none() {
        return Err(AppError::NotFound("文章不存在".into()));
    }
    posts::update_post(
        &state.db,
        id,
        posts::UpdatePost {
            title: None,
            content_md: Some(form.content_md),
            excerpt: None,
            slug: None,
            status: None,
            post_type: None,
            category_id: None,
            column_id: None,
            tags: None,
        },
    )
    .await?;
    Ok(Json(json!({
        "data": { "ok": true, "saved_at": super::format_local(Utc::now()) }
    })))
}

// ---------- 表单解析辅助 ----------

fn parse_status(s: &str) -> PostStatus {
    if s == "published" {
        PostStatus::Published
    } else {
        PostStatus::Draft
    }
}

/// 解析文章类型：`page` → 页面，其余 → 文章。
fn parse_post_type(s: &str) -> PostType {
    if s == "page" {
        PostType::Page
    } else {
        PostType::Post
    }
}

/// 空串视为 None（create 走服务层生成 excerpt/slug，update 视为不变）。
fn optional_field(v: Option<&String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn parse_id(v: Option<&String>) -> Option<i64> {
    v.and_then(|s| s.trim().parse::<i64>().ok())
}

/// 逗号分隔 → 标签名列表；空串 → 空数组。
fn parse_tags(v: Option<&String>) -> Vec<String> {
    v.map(|s| {
        s.split(',')
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect()
    })
    .unwrap_or_default()
}

fn categories_value(cats: &[Category]) -> Value {
    json!(cats
        .iter()
        .map(|c| json!({ "id": c.id, "slug": c.slug, "name": c.name }))
        .collect::<Vec<_>>())
}

fn columns_value(cols: &[crate::models::Column]) -> Value {
    json!(cols
        .iter()
        .map(|c| json!({ "id": c.id, "slug": c.slug, "name": c.name }))
        .collect::<Vec<_>>())
}
