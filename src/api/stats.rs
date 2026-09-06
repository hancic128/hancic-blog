//! REST API 统计端点：GET /api/stats/summary。
//!
//! `from`/`to` 为可选 `YYYY-MM-DD`（站点时区日期，当日边界）阅读明细范围；
//! 缺省不设限；趋势按站点时区自然日分组。响应 `{data: StatsSummary}`。

use crate::api;
use crate::error::AppError;
use crate::services::stats;
use crate::AppState;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::Json;
use serde_json::{Value, json};
use std::collections::HashMap;
use tower_sessions::Session;

/// GET /api/stats/summary：阅读汇总（总量 + 每日趋势）。
pub async fn summary(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    let tz = crate::services::timezone::site_timezone(&state.db).await;
    let summary = stats::summary(
        &state.db,
        query.get("from").map(String::as_str),
        query.get("to").map(String::as_str),
        &tz,
    )
    .await?;
    Ok(Json(json!({ "data": summary })))
}
