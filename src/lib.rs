pub mod admin;
pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod markdown;
pub mod models;
pub mod services;
pub mod session;
pub mod themes;
pub mod web;

use crate::config::Config;
use crate::error::AppError;
use crate::session::LoginLimiter;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use tera::Tera;

pub async fn app(config: Config) -> Result<Router, AppError> {
    let db = db::init(&config.data_dir).await?;
    session::migrate(&db).await?;
    // 主题加载失败（如全新部署尚未安装主题）时回退空 Tera 并告警，渲染侧在 T7 接入。
    let themes_dir = config.data_dir.join("themes");
    let (tera, theme_dir) = match themes::build_tera(&themes_dir, &config.active_theme) {
        Ok(tera) => (tera, themes_dir.join(&config.active_theme)),
        Err(err) => {
            tracing::warn!(
                "主题 {} 加载失败，回退空 Tera: {err}",
                config.active_theme
            );
            (Tera::default(), themes_dir.join(&config.active_theme))
        }
    };
    let state = AppState {
        config: Arc::new(config),
        db: db.clone(),
        login_limiter: Arc::new(LoginLimiter::new()),
        tera,
        theme_dir,
    };
    Ok(Router::new()
        .route("/api/health", get(health))
        .nest("/admin", admin::router())
        .merge(web::front::routes())
        .fallback(web::front::not_found)
        .layer(session::session_layer(&db))
        .with_state(state))
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: db::Db,
    pub login_limiter: Arc<LoginLimiter>,
    pub tera: Tera,
    pub theme_dir: PathBuf,
}

async fn health() -> Json<Value> {
    Json(json!({ "data": { "status": "ok" } }))
}
