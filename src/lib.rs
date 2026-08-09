pub mod admin;
pub mod api;
pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod ipregion;
pub mod markdown;
pub mod models;
pub mod services;
pub mod session;
pub mod themes;
pub mod web;

use crate::config::Config;
use crate::error::AppError;
use crate::session::LoginLimiter;
use axum::Router;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tera::Tera;

pub async fn app(config: Config) -> Result<Router, AppError> {
    let db = db::init(&config.data_dir).await?;
    session::migrate(&db).await?;
    // config 构造 AppState 时被 move，先备份数据目录供 ip 搜索器初始化使用。
    let db_data_dir = config.data_dir.clone();
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
    // /api 上传接受最大文件上限（100MB 视频）+ multipart 边界/字段头开销余量。
    let max_upload = config
        .upload_max_video
        .max(config.upload_max_image)
        .max(config.upload_max_file);
    let body_limit = max_upload + 2 * 1024 * 1024;
    let state = AppState {
        config: Arc::new(config),
        db: db.clone(),
        login_limiter: Arc::new(LoginLimiter::new()),
        tera,
        theme_dir,
        ip_searcher: Arc::new(init_ip_searcher(&db_data_dir)?),
    };
    Ok(Router::new()
        .nest(
            "/api",
            api::router().layer(axum::extract::DefaultBodyLimit::max(body_limit as usize)),
        )
        .nest("/admin", admin::router())
        .merge(web::front::routes())
        .fallback(web::front::not_found)
        .layer(session::session_layer(&db))
        .with_state(state))
}

/// 启动 IP 搜索器：确保 xdb 落盘后加载。失败直接阻断启动（地区统计属核心能力）。
fn init_ip_searcher(data_dir: &Path) -> Result<ipregion::Searcher, AppError> {
    let xdb = ipregion::ensure_xdb(data_dir).map_err(AppError::Internal)?;
    ipregion::Searcher::new(&xdb).map_err(AppError::Internal)
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: db::Db,
    pub login_limiter: Arc<LoginLimiter>,
    pub tera: Tera,
    pub theme_dir: PathBuf,
    pub ip_searcher: Arc<ipregion::Searcher>,
}
