pub mod admin;
pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod models;
pub mod services;
pub mod session;

use crate::config::Config;
use crate::error::AppError;
use crate::session::LoginLimiter;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};
use std::sync::Arc;

pub async fn app(config: Config) -> Result<Router, AppError> {
    let db = db::init(&config.data_dir).await?;
    session::migrate(&db).await?;
    let state = AppState {
        config: Arc::new(config),
        db: db.clone(),
        login_limiter: Arc::new(LoginLimiter::new()),
    };
    Ok(Router::new()
        .route("/api/health", get(health))
        .nest("/admin", admin::router())
        .layer(session::session_layer(&db))
        .with_state(state))
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: db::Db,
    pub login_limiter: Arc<LoginLimiter>,
}

async fn health() -> Json<Value> {
    Json(json!({ "data": { "status": "ok" } }))
}
