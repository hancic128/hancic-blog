//! 附件上传接口：POST /api/uploads（multipart 字段名 `files`）。
//!
//! 鉴权：本任务仅接受 admin 会话（`require_admin`）；Bearer token 校验由 T21 接入。

use crate::AppState;
use crate::error::AppError;
use crate::session;
use axum::extract::{Multipart, State};
use axum::Json;
use serde_json::{Value, json};
use tower_sessions::Session;

pub async fn upload(
    State(state): State<AppState>,
    session: Session,
    multipart: Multipart,
) -> Result<Json<Value>, AppError> {
    session::require_admin(&session).await?;
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
