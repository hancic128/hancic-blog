//! REST API 说说端点：POST /api/moments、DELETE /api/moments/{id}。
//!
//! 创建必填 `content`（非空字符串），可选 `attachment_ids`（整数数组，
//! 每个 id 必须指向存在的附件，否则 400）。响应 `201 {data: Moment}`，
//! 删除成功 `204`（不存在 404）。

use crate::api;
use crate::error::AppError;
use crate::services::{moments, uploads};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde_json::{Value, json};
use tower_sessions::Session;

/// POST /api/moments：创建说说（可附带附件）。
pub async fn create(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
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
