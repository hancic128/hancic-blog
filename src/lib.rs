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
pub mod util;
pub mod web;

use crate::config::Config;
use crate::error::AppError;
use crate::services::likes::LikeRateLimiter;
use crate::services::tokens::PlainStore;
use crate::session::LoginLimiter;
use axum::Router;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tera::Tera;
use tower_http::services::ServeDir;

pub async fn app(config: Config) -> Result<Router, AppError> {
    let db = db::init(&config.data_dir).await?;
    session::migrate(&db).await?;
    // 后台切主题只写 settings.active_theme（C2）：启动以 DB 为准、优先于
    // config 默认值，使「重启后生效」真正生效（此前按 config.active_theme
    // 构建 tera，重启后永远切不回 DB 里激活的主题）。
    let mut config = config;
    if let Some(theme) = crate::services::settings::get(&db, "active_theme")
        .await
        .ok()
        .flatten()
    {
        if !theme.trim().is_empty() {
            config.active_theme = theme;
        }
    }
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
        like_rate_limiter: Arc::new(LikeRateLimiter::new()),
        tera,
        tera_admin: admin::build_tera(),
        theme_dir,
        ip_searcher: Arc::new(init_ip_searcher(&db_data_dir)?),
        token_plain: PlainStore::default(),
    };
    // 每日 00:00（Asia/Shanghai）自动全量备份，轮转保留最近 3 份
    spawn_daily_backup(state.config.data_dir.clone(), state.db.clone());
    // 后台前端资源（admin.css/admin.js/vendor/）以仓库 assets/ 为根，
    // 与源码一同发布；编译期路径保证 cargo test 等任意 cwd 下可用。
    // 容器部署时编译期路径（Docker builder 的 /build/assets）不存在，
    // 回退到可执行文件同目录的 assets/（镜像内为 /app/assets，T25）。
    let mut assets_dir = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/assets"));
    if !assets_dir.is_dir() {
        // 容器部署：编译期路径（Docker builder 的 /build/assets）不存在，
        // 回退到可执行文件同目录的 assets/（镜像内为 /app/assets）。
        let exe_assets = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("assets")));
        if let Some(dir) = exe_assets.filter(|d| d.is_dir()) {
            tracing::warn!(
                "编译期静态资源目录不存在（{}），回退到可执行文件同目录: {}",
                assets_dir.display(),
                dir.display()
            );
            assets_dir = dir;
        }
    }
    Ok(Router::new()
        .nest(
            "/api",
            api::router().layer(axum::extract::DefaultBodyLimit::max(body_limit as usize)),
        )
        .nest("/admin", admin::router())
        .nest_service(
            "/static",
            tower::ServiceBuilder::new()
                .layer(axum::middleware::from_fn(no_cache_static))
                .service(ServeDir::new(&assets_dir)),
        )
        .merge(web::front::routes())
        .fallback(web::front::not_found)
        .layer(session::session_layer(&db))
        .with_state(state))
}

/// 后台静态资源（admin.css/admin.js/vendor）不设长缓存：这些文件随版本热服务，
/// 浏览器缓存旧版会导致样式/脚本不一致。统一 `no-cache`（协商缓存，改后立即生效）。
async fn no_cache_static(
    req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let mut res = next.run(req).await;
    res.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-cache"),
    );
    res
}

/// 启动 IP 搜索器：确保 xdb 落盘后加载。失败直接阻断启动（地区统计属核心能力）。
fn init_ip_searcher(data_dir: &Path) -> Result<ipregion::Searcher, AppError> {
    let xdb = ipregion::ensure_xdb(data_dir).map_err(AppError::Internal)?;
    ipregion::Searcher::new(&xdb).map_err(AppError::Internal)
}

/// 每日 00:00（Asia/Shanghai）自动全量备份到 `data_dir/backups/`，轮转保留最近 3 份。
/// 每小时 tick 检查一次是否进入 00 点窗口；当天备份文件已存在则跳过（重启防重）。
/// 备份记录写入 `backup_logs`（kind='export'），后台备份页可见。
fn spawn_daily_backup(data_dir: PathBuf, db: db::Db) {
    tokio::spawn(async move {
        use chrono::Timelike;
        let backup_dir = data_dir.join("backups");
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(3600));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            let now = chrono::Utc::now().with_timezone(&chrono_tz::Asia::Shanghai);
            if now.hour() != 0 {
                continue;
            }
            let day = now.format("%Y%m%d").to_string();
            let file = backup_dir.join(format!("hancic-{day}.zip"));
            if file.exists() {
                continue; // 当天已备份
            }
            if let Err(e) = std::fs::create_dir_all(&backup_dir) {
                tracing::error!("自动备份：创建备份目录失败 {}: {e}", backup_dir.display());
                continue;
            }
            match crate::services::backup::export_all(&data_dir, &file).await {
                Ok(report) => {
                    tracing::info!(
                        "自动备份完成 {}（{} 字节，{} 文件）",
                        file.display(),
                        report.size,
                        report.counts.files
                    );
                    let _ = sqlx::query(
                        "INSERT INTO backup_logs(kind, status, size, files, note) \
                         VALUES ('export', 'ok', ?, ?, '自动备份')",
                    )
                    .bind(report.size as i64)
                    .bind(report.counts.files as i64)
                    .execute(&db)
                    .await;
                    // 轮转：仅保留最近 3 份自动备份
                    let mut files: Vec<PathBuf> = std::fs::read_dir(&backup_dir)
                        .into_iter()
                        .flatten()
                        .filter_map(|e| e.ok())
                        .map(|e| e.path())
                        .filter(|p| {
                            p.file_name()
                                .and_then(|n| n.to_str())
                                .is_some_and(|n| n.starts_with("hancic-") && n.ends_with(".zip"))
                        })
                        .collect();
                    files.sort();
                    while files.len() > 3 {
                        if let Some(oldest) = files.first() {
                            let _ = std::fs::remove_file(oldest);
                            files.remove(0);
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("自动备份失败: {e:?}");
                    let _ = sqlx::query(
                        "INSERT INTO backup_logs(kind, status, size, files, note) \
                         VALUES ('export', 'failed', 0, 0, ?)",
                    )
                    .bind(format!("自动备份失败: {e:?}"))
                    .execute(&db)
                    .await;
                }
            }
        }
    });
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: db::Db,
    pub login_limiter: Arc<LoginLimiter>,
    pub like_rate_limiter: Arc<LikeRateLimiter>,
    pub tera: Tera,
    pub tera_admin: Tera,
    pub theme_dir: PathBuf,
    pub ip_searcher: Arc<ipregion::Searcher>,
    pub token_plain: PlainStore,
}
