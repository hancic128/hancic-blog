//! API 路由：/api 下 JSON 接口（health、uploads；posts/moments 等后续任务挂载）。

pub mod uploads;

use crate::AppState;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/uploads", post(uploads::upload))
}

async fn health() -> Json<Value> {
    Json(json!({ "data": { "status": "ok" } }))
}
