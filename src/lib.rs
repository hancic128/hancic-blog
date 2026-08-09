pub mod config;
pub mod db;
pub mod error;

use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;
use crate::config::Config;

pub fn app(config: Config) -> Router {
    let state = AppState { config: Arc::new(config) };
    Router::new()
        .route("/api/health", get(health))
        .with_state(state)
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
}

async fn health() -> Json<Value> {
    Json(json!({ "data": { "status": "ok" } }))
}
