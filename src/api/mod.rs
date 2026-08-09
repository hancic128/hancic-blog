//! API 路由：/api 下 JSON 接口，统一响应 `{data}` / `{error:{code,message}}`。
//!
//! 鉴权：除 `/api/health`（存活探针，保持开放，供监控/部署探测）外，全部端点
//! 经 `require_admin_or_token`——后台 admin 会话或 `Authorization: Bearer <token>`
//! 二选一，都失败则 401。`GET /api/backup` 依赖 T22 备份服务，本任务不挂载。
//! 统一 JSON 错误体由 `AppError` 的 `IntoResponse` 产出。

pub mod auth;
pub mod categories;
pub mod moments;
pub mod posts;
pub mod stats;
pub mod uploads;

use crate::error::AppError;
use crate::{session, AppState};
use axum::http::HeaderMap;
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use serde_json::{Value, json};
use tower_sessions::Session;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/uploads", post(uploads::upload))
        .route("/posts", get(posts::list).post(posts::create))
        .route("/posts/{id}", get(posts::get).patch(posts::update).delete(posts::delete))
        .route("/moments", post(moments::create))
        .route("/moments/{id}", delete(moments::delete))
        .route("/categories", get(categories::list).post(categories::create))
        .route(
            "/categories/{id}",
            patch(categories::update).delete(categories::delete),
        )
        .route("/stats/summary", get(stats::summary))
}

/// 统一 API 鉴权：admin 会话通过则放行；否则校验 `Authorization: Bearer <token>`。
pub async fn require_admin_or_token(
    state: &AppState,
    session: &Session,
    headers: &HeaderMap,
) -> Result<(), AppError> {
    if session::require_admin(session).await.is_ok() {
        return Ok(());
    }
    auth::verify_bearer(state, headers).await
}

/// GET /api/health：存活探针（不鉴权）。
async fn health() -> Json<Value> {
    Json(json!({ "data": { "status": "ok" } }))
}
