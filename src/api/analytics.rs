//! REST API 访问分析端点：GET /api/analytics。
//!
//! 返回访问分析各维度（访客 / 设备 / 来源 / 关键词 / 热力 / 互链 / 单篇下钻 /
//! 实时流）的 JSON 汇总，供 MCP 与任何 Bearer 客户端消费。
//!
//! `from`/`to` 为可选 `YYYY-MM-DD`（站点时区日期，当日边界）；缺省不设限。
//! 与后台「访问分析」页同一套口径：**默认剔除爬虫**。
//! `include_bots=1` 时额外返回爬虫明细（爬虫量本就在 summary 里）。

use crate::AppState;
use crate::api;
use crate::error::AppError;
use crate::services::analytics;
use axum::Json;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use serde_json::{Value, json};
use std::collections::HashMap;
use tower_sessions::Session;

/// 单维度返回条数上限。
const LIMIT: i64 = 20;
/// 单篇下钻覆盖文章数。
const DRILLDOWN_POSTS: i64 = 10;
/// 实时流条数。
const RECENT_VIEWS: i64 = 30;

/// GET /api/analytics：访问分析汇总。
pub async fn report(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    api::require_admin_or_token(&state, &session, &headers).await?;
    let tz = crate::services::timezone::site_timezone(&state.db).await;
    let from = query.get("from").map(String::as_str);
    let to = query.get("to").map(String::as_str);

    let summary = analytics::visitor_summary(&state.db, from, to, &tz).await?;
    let daily = analytics::daily_visitors(&state.db, from, to, &tz).await?;
    let devices = analytics::by_device(&state.db, from, to, &tz).await?;
    let browsers = analytics::by_browser(&state.db, from, to, &tz).await?;
    let os = analytics::by_os(&state.db, from, to, &tz).await?;
    let domains = analytics::by_referer_domain(&state.db, from, to, &tz, LIMIT).await?;
    let paths = analytics::by_referer_path(&state.db, from, to, &tz, LIMIT).await?;
    let keywords = analytics::search_keywords(&state.db, from, to, &tz, LIMIT).await?;
    let heatmap = analytics::hourly_heatmap(&state.db, from, to, &tz).await?;
    let post_sources =
        analytics::post_source_breakdown(&state.db, from, to, &tz, DRILLDOWN_POSTS, 10).await?;
    let recent = analytics::recent_views(&state.db, &tz, RECENT_VIEWS).await?;

    let site_domain = site_domain_of(&state.config.site_url);
    let internal_links =
        analytics::internal_links(&state.db, from, to, &tz, &site_domain, LIMIT).await?;

    Ok(Json(json!({ "data": json!({
        "summary": summary,
        "daily": daily,
        "devices": devices,
        "browsers": browsers,
        "os": os,
        "referer_domains": domains,
        "referer_paths": paths,
        "keywords": keywords,
        "heatmap": heatmap,
        "internal_links": internal_links,
        "post_sources": post_sources,
        "recent": recent,
    }) })))
}

/// 从 `site_url` 取域名，供站内互链判定。
fn site_domain_of(site_url: &str) -> String {
    let trimmed = site_url.trim().trim_end_matches('/');
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    let host = without_scheme.split('/').next().unwrap_or(without_scheme);
    host.split(':').next().unwrap_or(host).to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::site_domain_of;

    #[test]
    fn site_domain_parsed() {
        assert_eq!(site_domain_of("https://hancic.site/"), "hancic.site");
        assert_eq!(
            site_domain_of("https://blog.example.com/x"),
            "blog.example.com"
        );
        assert_eq!(site_domain_of(""), "");
    }
}
