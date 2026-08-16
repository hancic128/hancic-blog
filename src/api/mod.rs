//! API 路由：/api 下 JSON 接口，统一响应 `{data}` / `{error:{code,message}}`。
//!
//! 鉴权：`/api/health`（存活探针）与公开点赞端点
//! `/api/likes/status`、`/api/likes/toggle` 保持开放；其余端点经
//! `require_admin_or_token`——后台 admin 会话或 `Authorization: Bearer <token>`
//! 二选一，都失败则 401。`GET /api/backup` 返回全量备份 zip（T22）。
//! 统一 JSON 错误体由 `AppError` 的 `IntoResponse` 产出。

pub mod auth;
pub mod backup;
pub mod categories;
pub mod likes;
pub mod moments;
pub mod posts;
pub mod stats;
pub mod tags;
pub mod uploads;

use crate::error::AppError;
use crate::{session, AppState};
use axum::extract::rejection::JsonRejection;
use axum::http::HeaderMap;
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use serde_json::{Value, json};
use tower_sessions::Session;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/likes/status", get(likes::status))
        .route("/likes/toggle", post(likes::toggle))
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
        .route("/tags", get(tags::list))
        .route("/tags/{id}", delete(tags::delete))
        .route("/stats/summary", get(stats::summary))
        .route("/backup", get(backup::backup))
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

/// Json 拒绝（非法 JSON / 缺 Content-Type）统一转 400 AppError，
/// 保证失败响应仍是统一 JSON 错误体（axum 默认会回 text/plain 400/415）。
pub(crate) fn valid_json(
    body: Result<Json<Value>, JsonRejection>,
) -> Result<Json<Value>, AppError> {
    body.map_err(|_| AppError::BadRequest("请求体必须是合法 JSON".into()))
}

/// GET /api/health：存活探针（不鉴权）。
async fn health() -> Json<Value> {
    Json(json!({ "data": { "status": "ok" } }))
}
