//! REST API 分类端点：GET/POST /api/categories、PATCH/DELETE /api/categories/{id}。
//!
//! 创建必填 `name`（非空），`slug` 缺省由名称 slugify，`sort_order` 缺省 0；
//! slug 唯一冲突 → 409。更新为 PATCH 语义：缺失字段沿用现值，`slug` 传空串
//! 视为从名称重新生成。响应 `{data: Category}`（POST 为 201），删除成功 204。

use crate::api;
use crate::error::AppError;
use crate::services::{posts, taxonomy};
use crate::AppState;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde_json::{Value, json};
use tower_sessions::Session;

/// GET /api/categories：全量分类（按 sort_order, id 排序）。
pub async fn list(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    let categories = taxonomy::list_categories(&state.db).await?;
    Ok(Json(json!({ "data": categories })))
}

/// POST /api/categories：创建分类。
pub async fn create(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    body: Result<Json<Value>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    let body = api::valid_json(body)?;
    let name = require_nonempty(&body, "name")?.to_string();
    let slug = match opt_str(&body, "slug") {
        Some(s) if !s.trim().is_empty() => s.to_string(),
        _ => posts::slugify(&name).await,
    };
    let sort_order = opt_i64(&body, "sort_order")?.unwrap_or(0);
    let category = taxonomy::create_category(&state.db, &name, &slug, sort_order).await?;
    Ok((StatusCode::CREATED, Json(json!({ "data": category }))))
}

/// PATCH /api/categories/{id}：部分字段更新（缺失字段沿用现值）。
pub async fn update(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Path(id): Path<i64>,
    body: Result<Json<Value>, JsonRejection>,
) -> Result<Json<Value>, AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    let body = api::valid_json(body)?;
    let old = taxonomy::get_category_by_id(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound("分类不存在".into()))?;
    let name = match opt_str(&body, "name") {
        Some(s) if !s.trim().is_empty() => s.to_string(),
        Some(_) => return Err(AppError::BadRequest("name 必须是非空字符串".into())),
        None => old.name,
    };
    let slug = match opt_str(&body, "slug") {
        Some(s) if !s.trim().is_empty() => s.to_string(),
        // 传空串视为按名称重新生成（与后台表单/创建路径一致）
        Some(_) => posts::slugify(&name).await,
        None => old.slug,
    };
    let sort_order = opt_i64(&body, "sort_order")?.unwrap_or(old.sort_order);
    // slug 冲突预检：update_category 直接写库不查重，撞 UNIQUE 约束会 500，
    // 提前比对其他分类，命中则 409（与 create 的 Conflict 语义一致）。
    if let Some(other) = taxonomy::get_category_by_slug(&state.db, &slug).await? {
        if other.id != id {
            return Err(AppError::Conflict("分类 slug 已存在".into()));
        }
    }
    let category = taxonomy::update_category(&state.db, id, &name, &slug, sort_order).await?;
    Ok(Json(json!({ "data": category })))
}

/// DELETE /api/categories/{id}：删除分类（成功 204，不存在 404）。
/// 关联文章的 `category_id` 由外键 `ON DELETE SET NULL` 置空，文章不删除。
pub async fn delete(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    taxonomy::delete_category(&state.db, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- 请求体辅助（与 posts.rs 同款约定） ----------

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
