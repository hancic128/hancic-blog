//! REST API 文章端点：POST/GET /api/posts、GET/PATCH/DELETE /api/posts/{id}。
//!
//! 请求体为 JSON 对象，字段可选性按各端点契约：
//! - 创建必填 `title`（非空）、`content_md`（存在）；`status` 限 `draft`/`published`；
//!   `category_id` 若提供必须指向存在的分类（否则 400）。
//! - 更新为 PATCH 语义：缺失字段不变；`"category_id": null` 清空分类、
//!   `"excerpt": ""` 或 `null` 清空摘要（对应服务层 `Option<Option<T>>` 约定）。
//!
//! 响应统一 `{data: Post}`（POST 为 201），Post 序列化保留 `content_md` 原文与
//! `excerpt`/`published_at`/`views`；错误统一 `{error:{code,message}}`。

use crate::api;
use crate::error::AppError;
use crate::models::{PostStatus, PostType};
use crate::services::{posts as service, taxonomy};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::Deserialize;
use serde_json::{Value, json};
use tower_sessions::Session;

/// GET /api/posts 查询参数（全部可选；`page` 从 1 起、`page_size` 上限 100）。
#[derive(Deserialize)]
pub struct ListParams {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub status: Option<String>,
    pub category: Option<String>,
    pub tag: Option<String>,
}

/// POST /api/posts：创建文章（含可选 slug/excerpt/status/category_id/tags）。
pub async fn create(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    let input = parse_new_post(&state, &body).await?;
    let post = service::create_post(&state.db, input).await?;
    Ok((StatusCode::CREATED, Json(json!({ "data": post }))))
}

/// GET /api/posts：分页列表，支持 status/category/tag 筛选。
pub async fn list(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Query(params): Query<ListParams>,
) -> Result<Json<Value>, AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(10).clamp(1, 100);
    let status = parse_status_opt(params.status.as_deref())?;
    let (items, total) = service::list_posts(
        &state.db,
        service::PostListOptions {
            status,
            post_type: Some(PostType::Post),
            category_slug: params.category.filter(|s| !s.is_empty()),
            tag_slug: params.tag.filter(|s| !s.is_empty()),
            page,
            page_size,
        },
    )
    .await?;
    Ok(Json(json!({ "data": { "items": items, "total": total } })))
}

/// GET /api/posts/{id}：文章详情（不存在 404）。
pub async fn get(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    let post = service::get_post(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound("文章不存在".into()))?;
    Ok(Json(json!({ "data": post })))
}

/// PATCH /api/posts/{id}：部分字段更新（缺失字段不变）。
pub async fn update(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    let input = parse_update_post(&state, &body).await?;
    let post = service::update_post(&state.db, id, input).await?;
    Ok(Json(json!({ "data": post })))
}

/// DELETE /api/posts/{id}：删除文章（成功 204，不存在 404）。
pub async fn delete(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    service::delete_post(&state.db, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- 请求体 → 服务层结构 ----------

/// 创建：按 JSON 字段类型逐项映射，非法类型/枚举/分类引用一律 400。
async fn parse_new_post(state: &AppState, body: &Value) -> Result<service::NewPost, AppError> {
    let title = require_nonempty(body, "title")?.to_string();
    let content_md = require_field(body, "content_md")?.to_string();
    let excerpt = opt_str(body, "excerpt").map(str::to_string);
    let slug = opt_str(body, "slug").map(str::to_string);
    let status = parse_status_opt(opt_str(body, "status"))?.unwrap_or(PostStatus::Draft);
    let category_id = match opt_i64(body, "category_id")? {
        Some(id) => {
            check_category(state, id).await?;
            Some(id)
        }
        None => None,
    };
    let tags = opt_string_array(body, "tags")?.unwrap_or_default();
    Ok(service::NewPost {
        title,
        content_md,
        excerpt,
        slug,
        status,
        post_type: PostType::Post,
        category_id,
        tags,
    })
}

/// 更新（PATCH）：缺失字段 → None（服务层不变）；显式 null/空串按「清空」映射。
async fn parse_update_post(
    state: &AppState,
    body: &Value,
) -> Result<service::UpdatePost, AppError> {
    let title = match opt_str(body, "title") {
        Some(t) if t.trim().is_empty() => {
            return Err(AppError::BadRequest("title 必须是非空字符串".into()));
        }
        Some(t) => Some(t.to_string()),
        None => None,
    };
    let content_md = opt_str(body, "content_md").map(str::to_string);
    // excerpt：缺失 → None（不变）；null 或空串 → Some(None)（清空）；其余设值。
    let excerpt = match body.get("excerpt") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(Value::String(s)) if s.is_empty() => Some(None),
        Some(Value::String(s)) => Some(Some(s.clone())),
        Some(_) => return Err(AppError::BadRequest("excerpt 必须是字符串".into())),
    };
    let slug = opt_str(body, "slug").map(str::to_string);
    let status = parse_status_opt(opt_str(body, "status"))?;
    // category_id：缺失 → None（不变）；null → Some(None)（清空）；整数需存在。
    let category_id = match body.get("category_id") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(Value::Number(n)) => {
            let id = n.as_i64().ok_or_else(|| {
                AppError::BadRequest("category_id 必须是整数".into())
            })?;
            check_category(state, id).await?;
            Some(Some(id))
        }
        Some(_) => return Err(AppError::BadRequest("category_id 必须是整数".into())),
    };
    let tags = opt_string_array(body, "tags")?;
    Ok(service::UpdatePost {
        title,
        content_md,
        excerpt,
        slug,
        status,
        post_type: None,
        category_id,
        tags,
    })
}

/// `status` 字符串 → 枚举；`None` 放行（默认/不变），非法值 400。
fn parse_status_opt(s: Option<&str>) -> Result<Option<PostStatus>, AppError> {
    match s {
        None => Ok(None),
        Some("draft") => Ok(Some(PostStatus::Draft)),
        Some("published") => Ok(Some(PostStatus::Published)),
        Some(other) => Err(AppError::BadRequest(format!(
            "status 只能是 draft 或 published，收到: {other}"
        ))),
    }
}

/// 字段必须为非空字符串，否则 400。
fn require_nonempty<'a>(body: &'a Value, key: &str) -> Result<&'a str, AppError> {
    match body.get(key) {
        Some(Value::String(s)) if !s.trim().is_empty() => Ok(s),
        _ => Err(AppError::BadRequest(format!("{key} 必须是非空字符串"))),
    }
}

/// 字段必须存在（可为空串），否则 400。
fn require_field<'a>(body: &'a Value, key: &str) -> Result<&'a str, AppError> {
    match body.get(key) {
        Some(Value::String(s)) => Ok(s),
        _ => Err(AppError::BadRequest(format!("{key} 不能为空"))),
    }
}

/// 可选字符串字段；非字符串类型视为缺失（严格类型校验由各字段语义承担）。
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

/// 可选字符串数组字段；出现但含非字符串元素 → 400。
fn opt_string_array(body: &Value, key: &str) -> Result<Option<Vec<String>>, AppError> {
    match body.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(arr)) => {
            let mut out = Vec::with_capacity(arr.len());
            for v in arr {
                match v {
                    Value::String(s) => out.push(s.clone()),
                    _ => return Err(AppError::BadRequest(format!("{key} 必须是字符串数组"))),
                }
            }
            Ok(Some(out))
        }
        Some(_) => Err(AppError::BadRequest(format!("{key} 必须是字符串数组"))),
    }
}

/// category_id 存在性校验：不存在 → 400（创建/更新共用）。
async fn check_category(state: &AppState, id: i64) -> Result<(), AppError> {
    if taxonomy::get_category_by_id(&state.db, id)
        .await?
        .is_some()
    {
        Ok(())
    } else {
        Err(AppError::BadRequest(format!("分类不存在: {id}")))
    }
}
