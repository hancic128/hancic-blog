pub mod config;
pub mod db;
pub mod error;

use crate::config::Config;
use crate::error::AppError;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;

pub async fn app(config: Config) -> Result<Router, AppError> {
    let db = db::init(&config.data_dir).await?;
    let state = AppState {
        config: Arc::new(config),
        db,
    };
    Ok(Router::new()
        .route("/api/health", get(health))
        .with_state(state))
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: db::Db,
}

async fn health() -> Json<Value> {
    Json(json!({ "data": { "status": "ok" } }))
}
