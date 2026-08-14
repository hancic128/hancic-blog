//! REST API 标签端点：GET /api/tags（全量+各标签文章数）、DELETE /api/tags/{id}。
//!
//! 标签无创建/更新端点：写入口沿用文章编辑（tags 数组自动建标签）；删除会级联
//! 清理文章-标签关联（post_tags ON DELETE CASCADE），文章本身不受影响。

use crate::api;
use crate::error::AppError;
use crate::services::taxonomy;
use crate::AppState;
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
