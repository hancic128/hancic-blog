//! REST API 徒步轨迹：GET /api/trails（列表）、GET /api/trails/{id}（详情）。
//!
//! 轨迹数据由后台 GPX 上传维护；列表按最近轨迹优先返回（字段含里程/爬升/
//! 时长等展示统计），详情额外可选完整坐标（`?with_coords=1`，可用于前端画线）。

use crate::api;
use crate::error::AppError;
use crate::models::Trail;
use crate::services::trails;
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use serde_json::{Value, json};
use std::collections::HashMap;
use tower_sessions::Session;

/// 轨迹 JSON：基础字段 + 展示用格式化字段（url 为前台详情页路由）。
fn trail_json(t: &Trail) -> Value {
    json!({
        "id": t.id,
        "name": t.name,
        "description": t.description,
        "url": format!("/trails/{}", t.id),
        "started_at": t.started_at.map(|d| d.date_naive().to_string()),
        "distance_m": t.distance_m,
        "distance_km_str": t.distance_m.map(|m| format!("{:.1}", m / 1000.0)),
        "elevation_gain_m": t.elevation_gain_m,
        "elevation_gain_str": t.elevation_gain_m.map(|e| format!("{e:.0}")),
        "elevation_loss_m": t.elevation_loss_m,
        "moving_seconds": t.moving_seconds,
        "moving_time": trails::format_moving(t.moving_seconds),
        "avg_speed_kmh": t.avg_speed_kmh,
        "avg_speed_str": t.avg_speed_kmh.map(|s| format!("{s:.1}")),
        "max_elevation_m": t.max_elevation_m,
        "min_elevation_m": t.min_elevation_m,
        "point_count": t.point_count,
    })
}

/// GET /api/trails：轨迹列表（按开始时间/创建序，最近优先）。
pub async fn list(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    let items = trails::list_trails(&state.db, trails::TrailSort::parse(None)).await?;
    Ok(Json(json!({
        "data": { "items": items.iter().map(trail_json).collect::<Vec<_>>(), "total": items.len() }
    })))
}

/// GET /api/trails/{id}：轨迹详情；`?with_coords=1` 附加完整坐标数组
/// `[[lat, lon, speed?], ...]`（speed 单位 km/h，可能为 null）。
pub async fn get(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    let trail = trails::get_trail(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound("轨迹不存在".into()))?;
    let mut data = trail_json(&trail);
    if query.get("with_coords").is_some_and(|v| v == "1") {
        let dir = state.config.data_dir.join("trails");
        let coords = trails::load_full_coords(&dir, id)
            .unwrap_or_default()
            .into_iter()
            .map(|(lat, lon, spd)| {
                json!([lat, lon, serde_json::Value::from(spd)])
            })
            .collect::<Vec<_>>();
        data["coords"] = json!(coords);
    }
    Ok(Json(json!({ "data": data })))
}
