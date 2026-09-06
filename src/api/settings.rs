//! REST API 站点设置：GET /api/settings（只读全量键值）。
//!
//! 设置表保存前台渲染所需的站点信息（名称/描述/Logo/导航/社交/页脚等），
//! 无敏感凭据（登录密码在独立的 auth 表，不在此）。写操作仍走后台设置页
//! （表单校验 + CSRF），本接口保持只读。

use crate::api;
use crate::error::AppError;
use crate::services::settings;
use crate::AppState;
use axum::http::HeaderMap;
use axum::Json;
use serde_json::{Value, json};
use tower_sessions::Session;

/// GET /api/settings：全部站点设置（键值 Map）。
pub async fn get(
    state: axum::extract::State<AppState>,
    session: Session,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    let map = settings::all(&state.db).await?;
    Ok(Json(json!({ "data": map })))
}
