//! REST API 专栏端点：GET/POST /api/columns、PATCH/DELETE /api/columns/{id}，
//! GET/POST /api/columns/{id}/posts、DELETE /api/columns/{id}/posts/{post_id}。
//!
//! 校验与后台表单一致：`name` 必填且 ≤8 字、`description` ≤50 字；`slug` 缺省
//! 由名称生成，创建后保留不改（避免前台 /columns/{slug} 链接失效）。文章的
//! 加入/移出通过 `posts::update_post` 的 `column_id` 字段实现，不触碰其他字段。

use crate::api;
use crate::error::AppError;
use crate::models::{PostStatus, PostType};
use crate::services::{columns, posts};
use crate::AppState;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde_json::{Value, json};
use std::collections::HashMap;
use tower_sessions::Session;

/// GET /api/columns：全量专栏，含各专栏已发布普通文章数。
pub async fn list(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    let cols = columns::list_columns(&state.db).await?;
    let counts = columns::count_columns_posts(&state.db).await?;
    let data: Vec<Value> = cols
        .iter()
        .map(|c| {
            json!({
                "id": c.id,
                "slug": c.slug,
                "name": c.name,
                "description": c.description,
                "sort_order": c.sort_order,
                "posts": counts.get(&c.id).copied().unwrap_or(0),
            })
        })
        .collect();
    Ok(Json(json!({ "data": data })))
}

/// POST /api/columns：创建专栏（name 必填 ≤8 字；slug 缺省自动生成）。
pub async fn create(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    body: Result<Json<Value>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    let body = api::valid_json(body)?;
    let name = require_nonempty(&body, "name")?;
    if name.trim().chars().count() > 8 {
        return Err(AppError::BadRequest("专栏名称最多 8 个字".into()));
    }
    let description = opt_str(&body, "description").unwrap_or("").to_string();
    if description.trim().chars().count() > 50 {
        return Err(AppError::BadRequest("专栏描述最多 50 个字".into()));
    }
    let slug = match opt_str(&body, "slug") {
        Some(s) if !s.trim().is_empty() => s.to_string(),
        _ => columns::slug_for(name).await,
    };
    let sort_order = opt_i64(&body, "sort_order")?.unwrap_or(0);
    let column = columns::create_column(&state.db, name.trim(), &slug, sort_order, description.trim()).await?;
    Ok((StatusCode::CREATED, Json(json!({ "data": column }))))
}

/// PATCH /api/columns/{id}：改名称/描述（缺失字段沿用现值；slug 不改）。
pub async fn update(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Path(id): Path<i64>,
    body: Result<Json<Value>, JsonRejection>,
) -> Result<Json<Value>, AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    let body = api::valid_json(body)?;
    let old = columns::get_column_by_id(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound("专栏不存在".into()))?;
    let name = match opt_str(&body, "name") {
        Some(s) if s.trim().is_empty() => {
            return Err(AppError::BadRequest("name 必须是非空字符串".into()));
        }
        Some(s) => {
            if s.trim().chars().count() > 8 {
                return Err(AppError::BadRequest("专栏名称最多 8 个字".into()));
            }
            s.trim().to_string()
        }
        None => old.name,
    };
    let description = match opt_str(&body, "description") {
        Some(s) => {
            if s.trim().chars().count() > 50 {
                return Err(AppError::BadRequest("专栏描述最多 50 个字".into()));
            }
            s.trim().to_string()
        }
        None => old.description,
    };
    let column = columns::update_column(&state.db, id, &name, &description).await?;
    Ok(Json(json!({ "data": column })))
}

/// DELETE /api/columns/{id}：删除专栏（成功 204，不存在 404）。
/// 关联文章 `column_id` 由外键 `ON DELETE SET NULL` 置空，文章不删除。
pub async fn delete(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    columns::delete_column(&state.db, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/columns/{id}/posts：专栏下已发布文章（分页，按更新时间倒序）。
pub async fn list_posts(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    let column = columns::get_column_by_id(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound("专栏不存在".into()))?;
    let page = parse_int_param(&query, "page", 1)?.max(1);
    let page_size = parse_int_param(&query, "page_size", 10)?.clamp(1, 100);
    let (items, total) = posts::list_posts(
        &state.db,
        posts::PostListOptions {
            status: Some(PostStatus::Published),
            post_type: Some(PostType::Post),
            category_slug: None,
            tag_slug: None,
            column_slug: Some(column.slug),
            month: None,
            sort: Some(posts::PostSort { field: "updated_at", asc: false }),
            page,
            page_size,
        },
    )
    .await?;
    Ok(Json(json!({ "data": { "items": items, "total": total } })))
}

/// POST /api/columns/{id}/posts：把文章加入专栏（body: `post_id`）。
pub async fn add_post(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Path(id): Path<i64>,
    body: Result<Json<Value>, JsonRejection>,
) -> Result<Json<Value>, AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    let body = api::valid_json(body)?;
    columns::get_column_by_id(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound("专栏不存在".into()))?;
    let post_id = match body.get("post_id").and_then(Value::as_i64) {
        Some(v) => v,
        None => return Err(AppError::BadRequest("post_id 必须是整数".into())),
    };
    if posts::get_post(&state.db, post_id)
        .await?
        .is_none()
    {
        return Err(AppError::BadRequest(format!("文章不存在: {post_id}")));
    }
    let post = posts::update_post(&state.db, post_id, posts::UpdatePost {
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
    .await?;
    Ok(Json(json!({ "data": post })))
}

/// DELETE /api/columns/{id}/posts/{post_id}：把文章移出专栏（文章保留，成功 204）。
pub async fn remove_post(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Path((id, post_id)): Path<(i64, i64)>,
) -> Result<StatusCode, AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    columns::get_column_by_id(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound("专栏不存在".into()))?;
    posts::update_post(&state.db, post_id, posts::UpdatePost {
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
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- 请求体辅助（与 categories.rs/posts.rs 同款约定） ----------

/// 字段必须为非空字符串，否则 400。
fn require_nonempty<'a>(body: &'a Value, key: &str) -> Result<&'a str, AppError> {
    match body.get(key) {
        Some(Value::String(s)) if !s.trim().is_empty() => Ok(s),
        _ => Err(AppError::BadRequest(format!("{key} 必须是非空字符串"))),
    }
}

/// 可选字符串字段；非字符串类型视为缺失。
fn opt_str<'a>(body: &'a Value, key: &str) -> Option<&'a str> {
    match body.get(key) {
        Some(Value::String(s)) => Some(s),
        _ => None,
    }
}

/// 可选整数字段；出现但非整数 → 400。
fn opt_i64(body: &Value, key: &str) -> Result<Option<i64>, AppError> {
    match body.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => n.as_i64().map(Some).ok_or_else(|| {
            AppError::BadRequest(format!("{key} 必须是整数"))
        }),
        Some(_) => Err(AppError::BadRequest(format!("{key} 必须是整数"))),
    }
}

/// 查询参数转整数：缺省返回默认值，非法值 400（统一 JSON 错误体）。
fn parse_int_param(
    query: &HashMap<String, String>,
    key: &str,
    default: i64,
) -> Result<i64, AppError> {
    match query.get(key) {
        None => Ok(default),
        Some(s) => s
            .parse::<i64>()
            .map_err(|_| AppError::BadRequest(format!("{key} 必须是整数"))),
    }
}
