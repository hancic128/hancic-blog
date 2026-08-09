//! 附件上传接口：POST /api/uploads（multipart 字段名 `files`）。
//!
//! 鉴权：后台 admin 会话或 Bearer API Token 二选一（`api::require_admin_or_token`）。

use crate::api;
use crate::AppState;
use crate::error::AppError;
use axum::extract::{Multipart, State};
use axum::http::HeaderMap;
use axum::Json;
use serde_json::{Value, json};
use tower_sessions::Session;

pub async fn upload(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Json<Value>, AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    let uploads_dir = state.config.data_dir.join("uploads");
    let attachments = crate::services::uploads::save_upload_multipart(
        &state.db,
        &state.config,
        &uploads_dir,
        multipart,
    )
    .await?;
    Ok(Json(json!({ "data": attachments })))
}
