//! REST API 标签端点：GET /api/tags（全量+各标签文章数）、POST /api/tags（创建）、
//! DELETE /api/tags/{id}。
//!
//! 创建按 slug 幂等：同名标签返回既有记录（不重复创建）；删除会级联清理
//! 文章-标签关联（post_tags ON DELETE CASCADE），文章本身不受影响。

use crate::api;
use crate::error::AppError;
use crate::services::taxonomy;
use crate::AppState;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde_json::{Value, json};
use tower_sessions::Session;

/// GET /api/tags：全量标签，含各标签已发布普通文章数。
pub async fn list(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    let tags = taxonomy::list_tags(&state.db).await?;
    let counts = taxonomy::count_tags_posts(&state.db).await?;
    let data: Vec<Value> = tags
        .iter()
        .map(|t| {
            json!({
                "id": t.id,
                "slug": t.slug,
                "name": t.name,
                "posts": counts.get(&t.id).copied().unwrap_or(0),
            })
        })
        .collect();
    Ok(Json(json!({ "data": data })))
}

/// POST /api/tags：创建标签（name 必填 ≤5 字；同名幂等返回既有记录）。
pub async fn create(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    body: Result<Json<Value>, JsonRejection>,
) -> Result<Json<Value>, AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    let body = api::valid_json(body)?;
    let name = match body.get("name") {
        Some(Value::String(s)) if !s.trim().is_empty() => s.trim().to_string(),
        _ => return Err(AppError::BadRequest("name 必须是非空字符串".into())),
    };
    if name.chars().count() > 5 {
        return Err(AppError::BadRequest("标签名称最多 5 个字".into()));
    }
    let tag = taxonomy::ensure_tag(&state.db, &name).await?;
    Ok(Json(json!({ "data": { "id": tag.id, "slug": tag.slug, "name": tag.name } })))
}

/// DELETE /api/tags/{id}：删除标签（关联文章不受影响），成功 204。
pub async fn delete(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    taxonomy::delete_tag(&state.db, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
