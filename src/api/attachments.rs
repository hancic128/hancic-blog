//! REST API 附件：GET /api/attachments（列表，可筛选/分页）。
//!
//! 列表参数：`kind`（image/video/file，缺省全部）、`order`（asc/desc，按时间）、
//! `q`（文件名关键词）、`page` / `page_size`（默认 10，上限 100）。
//! 附件内容经前台 `/uploads/{path}` 公开静态路径访问（`url` 字段）。

use crate::api;
use crate::error::AppError;
use crate::models::AttachmentKind;
use crate::services::uploads;
use crate::AppState;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::Json;
use serde_json::{Value, json};
use std::collections::HashMap;
use tower_sessions::Session;

/// GET /api/attachments：附件列表（kind 筛选 + 关键词 + 时间排序 + 分页）。
pub async fn list(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    let page = query.get("page").and_then(|s| s.parse::<i64>().ok()).unwrap_or(1).max(1);
    let page_size = query
        .get("page_size")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(10)
        .clamp(1, 100);
    let asc = query.get("order").is_some_and(|s| s == "asc");
    let q = query.get("q").map(String::as_str);
    let kind = match query.get("kind").map(String::as_str).unwrap_or("") {
        "" => None,
        "image" => Some(AttachmentKind::Image),
        "video" => Some(AttachmentKind::Video),
        "file" => Some(AttachmentKind::File),
        other => return Err(AppError::BadRequest(format!("kind 必须是 image/video/file: {other}"))),
    };
    let (items, total) =
        uploads::list_attachments(&state.db, kind, asc, q, page, page_size).await?;
    let items: Vec<Value> = items
        .iter()
        .map(|a| {
            json!({
                "id": a.id,
                "kind": a.kind.to_str(),
                "orig_name": crate::util::percent_decode(&a.orig_name),
                "mime": a.mime,
                "size": a.size,
                "url": format!("{}/uploads/{}", state.config.base_path, a.path),
                "created_at": a.created_at,
            })
        })
        .collect();
    Ok(Json(json!({ "data": { "items": items, "total": total, "page": page, "page_size": page_size } })))
}
