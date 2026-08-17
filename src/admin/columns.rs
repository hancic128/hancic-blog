//! 后台专栏管理：列表/新建/编辑/删除 + 专栏下文章增减。
//!
//! 文章增减通过 `posts::update_post` 的 `column_id` 字段实现：
//! 添加 = 设值（Some(Some(id))），移除 = 清空（Some(None)），不触碰文章其他字段。

use crate::error::AppError;
use crate::models::{PostStatus, PostType};
use crate::services::{columns, posts};
use crate::{session, AppState};
use axum::extract::{Form, OriginalUri, Path, Query, State};
use axum::response::Response;
use serde_json::{Value, json};
use std::collections::HashMap;
use tower_sessions::Session;

/// 详情页可添加文章列表上限（全部已发布文章，过滤已在专栏的）。
const ADD_POST_LIMIT: i64 = 500;
/// 列表页每专栏文章显示上限（超出提示"查看全部"跳详情页）。
const COLUMN_LIST_POSTS: i64 = 15;

// ---------- 列表 ----------

pub async fn list(
    State(state): State<AppState>,
    session: Session,
    Query(query): Query<HashMap<String, String>>,
    uri: OriginalUri,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect(&state.config.base_path, "/admin/login");
    }
    // 文章排序：updated_at（默认）/ created_at / title
    // 专栏内文章默认按自定义顺序（column_sort asc）；显式 ?sort= 才用通用字段（desc）
    let (sort_field, asc) = match query.get("sort").map(String::as_str) {
        Some("created_at") => ("created_at", false),
        Some("title") => ("title", false),
        Some("updated_at") => ("updated_at", false),
        _ => ("column_sort", true),
    };
    let sort = posts::PostSort { field: sort_field, asc };
    let cols = columns::list_columns(&state.db).await.unwrap_or_default();
    let counts = columns::count_columns_posts(&state.db).await.unwrap_or_default();
    // 全部已发布文章（一次查询），供各专栏"添加文章"差集
    let (all_posts, _) = posts::list_posts(
        &state.db,
        posts::PostListOptions {
            status: Some(PostStatus::Published),
            post_type: Some(PostType::Post),
            category_slug: None,
            tag_slug: None,
            column_slug: None,
            month: None,
            sort: Some(posts::PostSort { field: "updated_at", asc: false }),
            page: 1,
            page_size: 500,
        },
    )
    .await
    .unwrap_or_default();
    let mut cards = Vec::with_capacity(cols.len());
    for c in &cols {
        let (in_posts, in_total) = posts::list_posts(
            &state.db,
            posts::PostListOptions {
                status: Some(PostStatus::Published),
                post_type: Some(PostType::Post),
                category_slug: None,
                tag_slug: None,
                column_slug: Some(c.slug.clone()),
                month: None,
                sort: Some(sort),
                page: 1,
                page_size: COLUMN_LIST_POSTS,
            },
        )
        .await
        .unwrap_or_default();
        let in_ids: std::collections::HashSet<i64> = in_posts.iter().map(|p| p.id).collect();
        cards.push(json!({
            "id": c.id,
            "slug": c.slug,
            "name": c.name,
            "description": c.description,
            "count": counts.get(&c.id).copied().unwrap_or(0),
            "in_total": in_total,
            "in_posts": in_posts.iter().map(|p| json!({
                "id": p.id,
                "title": p.title,
                "created_at": super::format_local(p.created_at),
                "updated_at": super::format_local(p.updated_at),
            })).collect::<Vec<_>>(),
            "addable": all_posts.iter().filter(|p| !in_ids.contains(&p.id)).map(|p| json!({ "id": p.id, "title": p.title })).collect::<Vec<_>>(),
        }));
    }
    let (mut ctx, csrf) = super::base_ctx(&state, &session, uri.path()).await;
    ctx.insert("columns", &json!(cards));
    ctx.insert("current_sort", &sort_field);
    ctx.insert("csrf", &csrf);
    ctx.insert(
        "error_msg",
        &uri.query()
            .and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("msg=")))
            .map(crate::util::percent_decode)
            .unwrap_or_default(),
    );
    super::render_admin(&state, "columns.html", &ctx)
}

// ---------- 详情（文章管理） ----------

pub async fn detail(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<i64>,
    uri: OriginalUri,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect(&state.config.base_path, "/admin/login");
    }
    let out = async {
        let column = columns::list_columns(&state.db)
            .await?
            .into_iter()
            .find(|c| c.id == id)
            .ok_or_else(|| AppError::NotFound("专栏不存在".into()))?;
        // 专栏下文章（按更新时间倒序）
        let (in_posts, _) = posts::list_posts(
            &state.db,
            posts::PostListOptions {
                status: Some(PostStatus::Published),
                post_type: Some(PostType::Post),
                category_slug: None,
                tag_slug: None,
                column_slug: Some(column.slug.clone()),
                month: None,
                sort: Some(posts::PostSort { field: "column_sort", asc: true }),
                page: 1,
                page_size: COLUMN_POSTS_LIMIT,
            },
        )
        .await?;
        // 全部已发布文章（供"添加文章"选择），过滤已在专栏的
        let (all_posts, _) = posts::list_posts(
            &state.db,
            posts::PostListOptions {
                status: Some(PostStatus::Published),
                post_type: Some(PostType::Post),
                category_slug: None,
                tag_slug: None,
                column_slug: None,
                month: None,
                sort: Some(posts::PostSort { field: "updated_at", asc: false }),
                page: 1,
                page_size: ADD_POST_LIMIT,
            },
        )
        .await?;
        let in_ids: std::collections::HashSet<i64> = in_posts.iter().map(|p| p.id).collect();
        let addable: Vec<Value> = all_posts
            .iter()
            .filter(|p| !in_ids.contains(&p.id))
            .map(|p| json!({ "id": p.id, "title": p.title }))
            .collect();
        let (mut ctx, csrf) = super::base_ctx(&state, &session, uri.path()).await;
        ctx.insert("column", &json!({ "id": column.id, "slug": column.slug, "name": column.name, "description": column.description }));
        ctx.insert(
            "in_posts",
            &json!(in_posts
                .iter()
                .map(|p| json!({ "id": p.id, "title": p.title, "url": format!("/admin/posts/{}/edit", p.id) }))
                .collect::<Vec<_>>()),
        );
        ctx.insert("addable_posts", &json!(addable));
        ctx.insert("csrf", &csrf);
        Ok::<_, AppError>(ctx)
    }
    .await;
    match out {
        Ok(ctx) => super::render_admin(&state, "column_posts.html", &ctx),
        Err(e) => {
            tracing::error!("专栏详情渲染失败: {e:?}");
            super::redirect(&state.config.base_path, "/admin/columns")
        }
    }
}

const COLUMN_POSTS_LIMIT: i64 = 500;

// ---------- 新建 ----------

pub async fn create(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<std::collections::HashMap<String, String>>,
) -> Result<Response, AppError> {
    session::require_admin(&session).await?;
    session::verify_csrf(&session, form.get("csrf").map(String::as_str)).await?;
    let name = form.get("name").cloned().unwrap_or_default();
    let description = form.get("description").cloned().unwrap_or_default();
    if let Some(msg) = validate(&name, &description) {
        return Ok(fail(&state.config.base_path, &msg));
    }
    let slug = columns::slug_for(&name).await;
    match columns::create_column(&state.db, &name, &slug, 0, &description).await {
        Ok(_) => Ok(super::redirect(&state.config.base_path, "/admin/columns")),
        Err(AppError::Conflict(_)) => Ok(fail(&state.config.base_path, "专栏 slug 已存在")),
        Err(e) => {
            tracing::error!("创建专栏失败: {e:?}");
            Ok(fail(&state.config.base_path, "创建专栏失败，请重试"))
        }
    }
}

// ---------- 编辑（名称/描述） ----------

pub async fn update(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<i64>,
    Form(form): Form<std::collections::HashMap<String, String>>,
) -> Result<Response, AppError> {
    session::require_admin(&session).await?;
    session::verify_csrf(&session, form.get("csrf").map(String::as_str)).await?;
    let name = form.get("name").cloned().unwrap_or_default();
    let description = form.get("description").cloned().unwrap_or_default();
    if let Some(msg) = validate(&name, &description) {
        return Ok(fail(&state.config.base_path, &msg));
    }
    match columns::update_column(&state.db, id, &name, &description).await {
        Ok(_) => Ok(super::redirect(&state.config.base_path, "/admin/columns")),
        Err(e) => {
            tracing::error!("更新专栏失败: {e:?}");
            Ok(fail(&state.config.base_path, "更新专栏失败，请重试"))
        }
    }
}

// ---------- 删除 ----------

pub async fn delete(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<i64>,
    Form(form): Form<std::collections::HashMap<String, String>>,
) -> Result<Response, AppError> {
    session::require_admin(&session).await?;
    session::verify_csrf(&session, form.get("csrf").map(String::as_str)).await?;
    match columns::delete_column(&state.db, id).await {
        Ok(()) => Ok(super::redirect(&state.config.base_path, "/admin/columns")),
        Err(e) => {
            tracing::error!("删除专栏失败: {e:?}");
            Ok(fail(&state.config.base_path, "删除专栏失败，请重试"))
        }
    }
}

// ---------- 文章增减 ----------

/// 往专栏添加文章（设置 column_id）。
pub async fn add_post(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<i64>,
    Form(form): Form<std::collections::HashMap<String, String>>,
) -> Result<Response, AppError> {
    session::require_admin(&session).await?;
    session::verify_csrf(&session, form.get("csrf").map(String::as_str)).await?;
    let post_id = match form.get("post_id").and_then(|v| v.trim().parse::<i64>().ok()) {
        Some(v) => v,
        None => return Ok(fail(&state.config.base_path, "请选择要添加的文章")),
    };
    // 校验专栏存在
    if !columns::list_columns(&state.db).await?.into_iter().any(|c| c.id == id) {
        return Ok(fail(&state.config.base_path, "专栏不存在"));
    }
    match posts::update_post(&state.db, post_id, posts::UpdatePost {
        title: None,
        content_md: None,
        excerpt: None,
        slug: None,
        status: None,
        post_type: None,
        category_id: None,
        column_id: Some(Some(id)),
        tags: None,
    })
    .await
    {
        Ok(_) => Ok(super::redirect(
            &state.config.base_path,
            &format!("/admin/columns/{id}"),
        )),
        Err(_) => Ok(fail(&state.config.base_path, "添加文章失败，请重试")),
    }
}

/// 从专栏移除文章（column_id 置空，文章保留）。
pub async fn remove_post(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<i64>,
    Form(form): Form<std::collections::HashMap<String, String>>,
) -> Result<Response, AppError> {
    session::require_admin(&session).await?;
    session::verify_csrf(&session, form.get("csrf").map(String::as_str)).await?;
    let post_id = match form.get("post_id").and_then(|v| v.trim().parse::<i64>().ok()) {
        Some(v) => v,
        None => return Ok(fail(&state.config.base_path, "文章参数缺失")),
    };
    match posts::update_post(&state.db, post_id, posts::UpdatePost {
        title: None,
        content_md: None,
        excerpt: None,
        slug: None,
        status: None,
        post_type: None,
        category_id: None,
        column_id: Some(None),
        tags: None,
    })
    .await
    {
        Ok(_) => Ok(super::redirect(
            &state.config.base_path,
            &format!("/admin/columns/{id}"),
        )),
        Err(_) => Ok(fail(&state.config.base_path, "移除文章失败，请重试")),
    }
}

// ---------- 辅助 ----------

fn validate(name: &str, description: &str) -> Option<String> {
    if name.trim().is_empty() {
        return Some("专栏名称不能为空".into());
    }
    if name.trim().chars().count() > 8 {
        return Some("专栏名称最多 8 个字".into());
    }
    if description.trim().chars().count() > 50 {
        return Some("专栏描述最多 50 个字".into());
    }
    None
}

/// 302 回列表并带 URL 编码的错误提示。
fn fail(base: &str, msg: &str) -> Response {
    super::redirect(base, &format!("/admin/columns?msg={}", urlencode(msg)))
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ---------- 卡片拖拽排序 ----------

/// 专栏卡片拖拽排序：前端提交新顺序的 id 列表，后端重写 sort_order（1..n）。
pub async fn reorder(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<std::collections::HashMap<String, String>>,
) -> Result<Response, AppError> {
    session::require_admin(&session).await?;
    session::verify_csrf(&session, form.get("csrf").map(String::as_str)).await?;
    let ids = parse_ids(form.get("ids"));
    if ids.is_empty() {
        return Ok(fail(&state.config.base_path, "排序数据为空"));
    }
    match columns::reorder_columns(&state.db, &ids).await {
        Ok(()) => Ok(super::redirect(&state.config.base_path, "/admin/columns")),
        Err(e) => {
            tracing::error!("专栏排序失败: {e:?}");
            Ok(fail(&state.config.base_path, "排序保存失败，请重试"))
        }
    }
}

// ---------- 专栏内文章拖拽排序 ----------

/// 专栏内文章拖拽排序：重写 posts.column_sort（0..n）。
pub async fn reorder_posts(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<i64>,
    Form(form): Form<std::collections::HashMap<String, String>>,
) -> Result<Response, AppError> {
    session::require_admin(&session).await?;
    session::verify_csrf(&session, form.get("csrf").map(String::as_str)).await?;
    if !columns::list_columns(&state.db).await?.into_iter().any(|c| c.id == id) {
        return Ok(fail(&state.config.base_path, "专栏不存在"));
    }
    let ids = parse_ids(form.get("ids"));
    if ids.is_empty() {
        return Ok(fail(&state.config.base_path, "排序数据为空"));
    }
    match posts::reorder_column_posts(&state.db, &ids).await {
        Ok(()) => Ok(super::redirect(
            &state.config.base_path,
            &format!("/admin/columns/{id}"),
        )),
        Err(e) => {
            tracing::error!("专栏文章排序失败: {e:?}");
            Ok(fail(&state.config.base_path, "排序保存失败，请重试"))
        }
    }
}

fn parse_ids(s: Option<&String>) -> Vec<i64> {
    s.map(|v| {
        v.split(',')
            .filter_map(|x| x.trim().parse::<i64>().ok())
            .collect()
    })
    .unwrap_or_default()
}
