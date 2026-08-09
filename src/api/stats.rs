//! REST API 统计端点：GET /api/stats/summary。
//!
//! `from`/`to` 为可选 `YYYY-MM-DD`（UTC 日期，当日边界）阅读明细范围；
//! 缺省不设限。响应 `{data: StatsSummary}`。

use crate::api;
use crate::error::AppError;
use crate::services::stats;
use crate::AppState;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::Json;
use serde::Deserialize;
use serde_json::{Value, json};
use tower_sessions::Session;

/// GET /api/stats/summary 查询参数（均可选）。
#[derive(Deserialize)]
pub struct RangeParams {
    pub from: Option<String>,
    pub to: Option<String>,
}

/// GET /api/stats/summary：阅读汇总（总量 + 每日趋势）。
pub async fn summary(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Query(params): Query<RangeParams>,
) -> Result<Json<Value>, AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    let summary = stats::summary(
        &state.db,
        params.from.as_deref(),
        params.to.as_deref(),
    )
    .await?;
    Ok(Json(json!({ "data": summary })))
}
