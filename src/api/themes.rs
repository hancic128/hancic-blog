//! REST API 主题管理：GET /api/themes（列表 + 当前）、POST /api/themes/{name}/activate。
//!
//! 主题以 `data/themes/{name}` 目录存在（theme.toml 声明元信息）；「当前主题」
//! 存于 `settings.active_theme`（缺省回落配置默认值）。切换写入后前台模板
//! 需重启服务完全生效（与后台页面一致的口径）。

use crate::api;
use crate::error::AppError;
use crate::services::settings;
use crate::{themes, AppState};
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use serde_json::{Value, json};
use tower_sessions::Session;

/// GET /api/themes：已安装主题列表（含当前主题）。
pub async fn list(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    let themes_dir = state.config.data_dir.join("themes");
    let metas = themes::discover(&themes_dir).unwrap_or_default();
    let current = settings::get(&state.db, "active_theme")
        .await?
        .filter(|s| !s.is_empty())
        .unwrap_or(state.config.active_theme.clone());
    let items: Vec<Value> = metas
        .iter()
        .map(|m| {
            json!({
                "name": m.name,
                "author": m.author,
                "version": m.version,
                "description": m.description,
                "is_current": m.name == current,
            })
        })
        .collect();
    Ok(Json(json!({ "data": { "items": items, "current": current } })))
}

/// POST /api/themes/{name}/activate：把主题写为当前主题（需重启完全生效）。
pub async fn activate(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<Json<Value>, AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    // 主题必须真实存在（目录 + theme.toml），防止写入不存在的名字
    let themes_dir = state.config.data_dir.join("themes");
    if !themes::is_valid_name(&name) || themes::load_meta(&themes_dir, &name).is_err() {
        return Err(AppError::NotFound(format!("主题不存在: {name}")));
    }
    settings::set(&state.db, "active_theme", &name).await?;
    Ok(Json(json!({
        "data": {
            "name": name,
            "note": "已切换，重启服务后前台完全生效"
        }
    })))
}
