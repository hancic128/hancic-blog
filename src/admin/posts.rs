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

pub async fn list(
    State(state): State<AppState>,
    session: Session,
    Query(query): Query<HashMap<String, String>>,
    uri: OriginalUri,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect("/admin/login");
    }
    let status = match query.get("status").map(String::as_str).unwrap_or("") {
        "draft" => Some(PostStatus::Draft),
        "published" => Some(PostStatus::Published),
        _ => None,
    };
    let category_slug = query.get("category").filter(|s| !s.is_empty()).cloned();
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
                post_type: Some(PostType::Post),
                category_slug: category_slug.clone(),
                tag_slug: None,
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
        match list_by_keyword(&state.db, status, category_slug.as_deref(), &q, page).await {
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
            "category": category_slug.unwrap_or_default(),
            "q": q,
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
            "views": p.views,
            "category": p
                .category_id
                .and_then(|id| cat_names.get(&id))
                .cloned()
                .unwrap_or_default(),
            "updated_at": super::format_local(p.updated_at),
        }))
        .collect::<Vec<_>>())
}

/// 标题关键词 + 状态/分类组合查询（排序与 `list_posts` 一致）。
async fn list_by_keyword(
    db: &Db,
    status: Option<PostStatus>,
    category_slug: Option<&str>,
    q: &str,
    page: i64,
) -> Result<(Vec<Post>, i64), AppError> {
    let mut where_sql = String::from(" WHERE post_type = ? AND title LIKE ?");
    let mut binds: Vec<String> = vec![PostType::Post.to_str().to_string(), format!("%{q}%")];
    if let Some(st) = status {
        where_sql.push_str(" AND status = ?");
        binds.push(st.to_str().to_string());
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
        "SELECT {cols} FROM posts{where_sql} ORDER BY published_at DESC, id DESC LIMIT ? OFFSET ?",
        cols = posts::POST_COLUMNS
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
        return super::redirect("/admin/login");
    }
    render_new(&state, &session, uri.path(), "").await
}

/// 新建页上下文（创建失败回显用）。
async fn render_new(state: &AppState, session: &Session, path: &str, error_tip: &str) -> Response {
    let categories = taxonomy::list_categories(&state.db).await.unwrap_or_default();
    let (mut ctx, _csrf) = super::base_ctx(state, session, path).await;
    ctx.insert("post", &empty_post_value());
    ctx.insert("categories", &categories_value(&categories));
    ctx.insert("error_tip", error_tip);
    super::render_admin(state, "post_edit.html", &ctx)
}

/// 新建页的空文章 JSON：id=0 时 `window._post` 无 id，自动保存不生效。
fn empty_post_value() -> Value {
    json!({
        "id": 0,
        "title": "",
        "content_md": "",
        "slug": "",
        "status": PostStatus::Draft.to_str(),
        "excerpt": "",
        "tags": "",
        "category_id": 0,
    })
}

pub async fn edit_page(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<i64>,
    uri: OriginalUri,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect("/admin/login");
    }
    match render_edit(&state, &session, id, uri.path(), "").await {
        Ok(resp) => resp,
        Err(e) => {
            tracing::error!("渲染文章编辑页失败: {e:?}");
            super::redirect("/admin/posts")
        }
    }
}

/// 编辑页上下文（slug 冲突等错误回显用）。
async fn render_edit(
    state: &AppState,
    session: &Session,
    id: i64,
    path: &str,
    error_tip: &str,
) -> Result<Response, AppError> {
    let Some(post) = posts::get_post(&state.db, id).await? else {
        return Ok(super::redirect("/admin/posts"));
    };
    let tags = posts::list_tags_of_post(&state.db, id)
        .await
        .unwrap_or_default();
    let categories = taxonomy::list_categories(&state.db).await.unwrap_or_default();
    let (mut ctx, _csrf) = super::base_ctx(state, session, path).await;
    ctx.insert("post", &post_edit_value(&post, &tags));
    ctx.insert("categories", &categories_value(&categories));
    ctx.insert("error_tip", error_tip);
    Ok(super::render_admin(state, "post_edit.html", &ctx))
}

/// 编辑页文章 JSON：content_md 由模板 `{{ post.content_md }}` 进 `data-content`
/// 属性（tera autoescape 保证属性值安全），tags 拼成逗号分隔字符串回填输入框。
fn post_edit_value(p: &Post, tags: &[Tag]) -> Value {
    json!({
        "id": p.id,
        "title": p.title,
        "content_md": p.content_md,
        "slug": p.slug,
        "status": p.status.to_str(),
        "excerpt": p.excerpt,
        "tags": tags
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        "category_id": p.category_id.unwrap_or(0),
    })
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
        slug: optional_field(form.get("slug")),
        status: parse_status(form.get("status").map(String::as_str).unwrap_or("")),
        post_type: PostType::Post,
        category_id: parse_id(form.get("category_id")),
        tags: parse_tags(form.get("tags")),
    };
    match posts::create_post(&state.db, input).await {
        Ok(p) => Ok(super::redirect(&format!("/admin/posts/{}/edit", p.id))),
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
        return Ok(super::redirect("/admin/posts"));
    }
    // slug 冲突预检：`update_post` 直接写 slug，撞 UNIQUE 约束会 500，
    // 提前用 slugify 后的值比对，命中其他文章则回显编辑页。
    if let Some(slug) = optional_field(form.get("slug")) {
        let slug = posts::slugify(&slug).await;
        if let Some(other) = posts::get_post_by_slug(&state.db, &slug).await? {
            if other.id != id {
                return render_edit(
                    &state,
                    &session,
                    id,
                    "/admin/posts/{id}/edit",
                    "固定链接已被占用，请换一个",
                )
                .await;
            }
        }
    }
    let input = posts::UpdatePost {
        title: optional_field(form.get("title")),
        content_md: Some(form.get("content_md").cloned().unwrap_or_default()),
        excerpt: optional_field(form.get("excerpt")),
        slug: form.get("slug").cloned(), // 空串视为不变（T3 已修）
        status: Some(parse_status(form.get("status").map(String::as_str).unwrap_or(""))),
        post_type: None,
        category_id: parse_id(form.get("category_id")),
        tags: Some(parse_tags(form.get("tags"))),
    };
    let p = posts::update_post(&state.db, id, input).await?;
    Ok(super::redirect(&format!("/admin/posts/{}/edit", p.id)))
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
        Ok(()) => Ok(super::redirect("/admin/posts")),
        Err(e) => {
            tracing::error!("删除文章失败: {e:?}");
            Ok(super::redirect("/admin/posts"))
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
