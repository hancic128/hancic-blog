//! REST API 说说端点：GET /api/moments、POST /api/moments、
//! GET /api/moments/{id}、PATCH /api/moments/{id}、DELETE /api/moments/{id}。
//!
//! 创建必填 `content`（非空字符串），可选 `attachment_ids`（整数数组，
//! 每个 id 必须指向存在的附件，否则 400）。响应 `201 {data: Moment}`，
//! 删除成功 `204`（不存在 404）。更新为部分更新：`content` / `attachment_ids`
//! 只更新提供的字段（attachment_ids 传数组即整体替换，`[]` 表示清空附件）。

use crate::api;
use crate::error::AppError;
use crate::models::Moment;
use crate::services::{moments, uploads};
use crate::AppState;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde_json::{Value, json};
use std::collections::HashMap;
use tower_sessions::Session;

/// POST /api/moments：创建说说（可附带附件）。
pub async fn create(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    body: Result<Json<Value>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    let body = api::valid_json(body)?;
    let content = match body.get("content") {
        Some(Value::String(s)) if !s.trim().is_empty() => s.clone(),
        _ => return Err(AppError::BadRequest("content 必须是非空字符串".into())),
    };
    let attachment_ids = parse_id_array(&body, "attachment_ids")?;
    for id in &attachment_ids {
        if uploads::get_attachment(&state.db, *id).await?.is_none() {
            return Err(AppError::BadRequest(format!("附件不存在: {id}")));
        }
    }
    let moment = moments::create_moment(&state.db, &content, &attachment_ids).await?;
    Ok((StatusCode::CREATED, Json(json!({ "data": moment }))))
}

/// DELETE /api/moments/{id}：删除说说（成功 204，不存在 404）。
pub async fn delete(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    moments::delete_moment(&state.db, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/moments：说说列表（分页 + 关键词/月份/排序）。
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
    let month = query.get("month").map(String::as_str);
    let (items, total) =
        moments::list_moments(&state.db, month, asc, q, page, page_size).await?;
    let mut out = Vec::with_capacity(items.len());
    for m in &items {
        out.push(moment_json(&state, m).await?);
    }
    Ok(Json(json!({ "data": { "items": out, "total": total, "page": page, "page_size": page_size } })))
}

/// GET /api/moments/{id}：说说详情（含附件）。
pub async fn get(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    let moment = moments::get_moment(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound("说说不存在".into()))?;
    Ok(Json(json!({ "data": moment_json(&state, &moment).await? })))
}

/// PATCH /api/moments/{id}：部分更新（content 与 attachment_ids 只更提供的字段）。
pub async fn update(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Path(id): Path<i64>,
    body: Result<Json<Value>, JsonRejection>,
) -> Result<Json<Value>, AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    let body = api::valid_json(body)?;
    let existing = moments::get_moment(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound("说说不存在".into()))?;

    // content：提供则必须非空
    let new_content = match body.get("content") {
        None | Some(Value::Null) => existing.content.clone(),
        Some(Value::String(s)) if !s.trim().is_empty() => s.clone(),
        Some(_) => return Err(AppError::BadRequest("content 必须是非空字符串".into())),
    };
    // attachment_ids：null/缺省=不改；数组=整体替换（[] 清空）
    let new_ids = match body.get("attachment_ids") {
        None | Some(Value::Null) => {
            let current = moments::list_moment_attachments(&state.db, id).await?;
            current.iter().map(|(a, _)| a.id).collect::<Vec<_>>()
        }
        Some(_) => parse_id_array(&body, "attachment_ids")?,
    };
    for aid in &new_ids {
        if uploads::get_attachment(&state.db, *aid).await?.is_none() {
            return Err(AppError::BadRequest(format!("附件不存在: {aid}")));
        }
    }
    moments::update_moment_with_attachments(&state.db, id, &new_content, &new_ids).await?;
    let updated = moments::get_moment(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound("说说不存在".into()))?;
    Ok(Json(json!({ "data": moment_json(&state, &updated).await? })))
}

/// 说说 JSON：正文/时间/点赞 + 附件数组（url 指向前台 /uploads 静态路径）。
async fn moment_json(state: &AppState, m: &Moment) -> Result<Value, AppError> {
    let atts = moments::list_moment_attachments(&state.db, m.id).await?;
    let attachments: Vec<Value> = atts
        .iter()
        .map(|(a, _)| {
            json!({
                "id": a.id,
                "kind": a.kind.to_str(),
                "orig_name": crate::util::percent_decode(&a.orig_name),
                "mime": a.mime,
                "url": format!("{}/uploads/{}", state.config.base_path, a.path),
            })
        })
        .collect();
    Ok(json!({
        "id": m.id,
        "content": m.content,
        "created_at": m.created_at,
        "like_count": m.like_count,
        "attachments": attachments,
    }))
}

/// 可选整数数组字段；缺省为空数组，出现但含非整数元素 → 400。
fn parse_id_array(body: &Value, key: &str) -> Result<Vec<i64>, AppError> {
    match body.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(arr)) => {
            let mut out = Vec::with_capacity(arr.len());
            for v in arr {
                match v.as_i64() {
                    Some(id) => out.push(id),
                    None => return Err(AppError::BadRequest(format!("{key} 必须是整数数组"))),
                }
            }
            Ok(out)
        }
        Some(_) => Err(AppError::BadRequest(format!("{key} 必须是整数数组"))),
    }
}
