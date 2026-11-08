//! 后台「访问分析」页：真实访客 / 设备构成 / 来源明细 / 搜索关键词 /
//! 时段热力 / 站内互链 / 单篇下钻 / 实时访问流。
//!
//! 路由：
//!   GET /admin/analytics   访问分析页（需登录）
//!
//! 与仪表盘（`/admin`）的分工：仪表盘是**摘要**（总量、趋势、排行、地区、
//! 来源分类），本页是**维度下钻**（访客构成、设备、来源明细、关键词等）。
//!
//! 时间语义与仪表盘一致：`from`/`to` 为站点时区自然日，缺省近 30 天；
//! `range=all` 表示不设限。**本页所有面向读者的指标默认剔除爬虫**
//! （`device != 'bot'`），爬虫量单独展示为一张卡——搜索引擎爬虫每日抓取
//! 会让原始 PV 虚高数倍，不剔除就没法判断「真人读了多少」。
//!
//! 鉴权约定同其他后台模块：未登录 302 跳登录。

use crate::error::AppError;
use crate::services::analytics;
// 区间解析与回填用后台的辅助函数（`admin::stats`），与仪表盘同一套语义
use crate::AppState;
use crate::admin::stats as admin_stats;
use axum::extract::{OriginalUri, Query, State};
use axum::response::Response;
use serde_json::json;
use std::collections::HashMap;
use tower_sessions::Session;

/// 来源域名明细显示条数。
const TOP_DOMAINS: i64 = 20;
/// 来源路径明细显示条数。
const TOP_PATHS: i64 = 20;
/// 搜索关键词显示条数。
const TOP_KEYWORDS: i64 = 20;
/// 单篇下钻覆盖的文章数。
const DRILLDOWN_POSTS: i64 = 10;
/// 站内互链显示条数。
const TOP_INTERNAL_LINKS: i64 = 20;
/// 实时访问流条数。
const RECENT_VIEWS: i64 = 30;

/// GET /admin/analytics：访问分析页。
pub async fn page(
    State(state): State<AppState>,
    session: Session,
    Query(query): Query<HashMap<String, String>>,
    uri: OriginalUri,
) -> Response {
    if crate::session::require_admin(&session).await.is_err() {
        return super::redirect(&state.config.base_path, "/admin/login");
    }
    let tz = crate::services::timezone::site_timezone(&state.db).await;
    let (from, to) = admin_stats::parse_range(&query, &tz).unwrap_or((None, None));
    let (mut ctx, _csrf) = super::base_ctx(&state, &session, uri.path()).await;

    // 区间语义与仪表盘一致：双 None = 「全部」不设限
    let all = from.is_none() && to.is_none();
    let (from_str, to_str) = if all {
        (String::new(), String::new())
    } else {
        admin_stats::effective_range(&from, &to, &tz)
    };
    ctx.insert("from", &from_str);
    ctx.insert("to", &to_str);
    ctx.insert("all", &all);

    if let Err(e) = fill(&state, &mut ctx, from.as_deref(), to.as_deref(), &tz).await {
        tracing::error!("访问分析数据查询失败: {e:?}");
    }
    super::render_admin(&state, "analytics.html", &ctx)
}

/// 填充访问分析页上下文。
///
/// **先写默认空值再覆盖**：这样任一查询失败都不会让模板因缺变量而渲染
/// 500——页面降级为「该维度暂无数据」，其余维度照常展示（分析页的数据
/// 来自多张表，一处失败不该拖垮整页）。
async fn fill(
    state: &AppState,
    ctx: &mut tera::Context,
    from: Option<&str>,
    to: Option<&str>,
    tz: &chrono_tz::Tz,
) -> Result<(), AppError> {
    // 默认值（后续查询成功则覆盖）
    ctx.insert(
        "summary",
        &json!({
            "total_views": 0,
            "unique_visitors": 0,
            "new_visitors": 0,
            "returning_visitors": 0,
            "bot_views": 0,
            "views_per_visitor": 0.0,
        }),
    );
    ctx.insert("devices", &Vec::<serde_json::Value>::new());
    ctx.insert("browsers", &Vec::<serde_json::Value>::new());
    ctx.insert("os_list", &Vec::<serde_json::Value>::new());
    ctx.insert("domains", &Vec::<serde_json::Value>::new());
    ctx.insert("paths", &Vec::<serde_json::Value>::new());
    ctx.insert("keywords", &Vec::<serde_json::Value>::new());
    ctx.insert("internal_links", &Vec::<serde_json::Value>::new());
    ctx.insert("post_sources", &Vec::<serde_json::Value>::new());
    ctx.insert("recent", &Vec::<serde_json::Value>::new());
    ctx.insert("bot_ratio", &0.0);
    ctx.insert("has_internal_links", &false);
    ctx.insert_value(
        "chart_data",
        tera::Value::safe_string(
            &json!({
                "labels": [], "views": [], "visitors": [],
                "devices": [], "browsers": [], "os": [], "heatmap": [],
            })
            .to_string(),
        ),
    );

    let summary = analytics::visitor_summary(&state.db, from, to, tz).await?;
    let daily = analytics::daily_visitors(&state.db, from, to, tz).await?;
    let devices = analytics::by_device(&state.db, from, to, tz).await?;
    let browsers = analytics::by_browser(&state.db, from, to, tz).await?;
    let os = analytics::by_os(&state.db, from, to, tz).await?;
    let domains = analytics::by_referer_domain(&state.db, from, to, tz, TOP_DOMAINS).await?;
    let paths = analytics::by_referer_path(&state.db, from, to, tz, TOP_PATHS).await?;
    let keywords = analytics::search_keywords(&state.db, from, to, tz, TOP_KEYWORDS).await?;
    let heatmap = analytics::hourly_heatmap(&state.db, from, to, tz).await?;
    let post_sources =
        analytics::post_source_breakdown(&state.db, from, to, tz, DRILLDOWN_POSTS, 10).await?;
    let recent = analytics::recent_views(&state.db, tz, RECENT_VIEWS).await?;

    // 站内互链需要本站域名：从 config.site_url 取 host（无配置则跳过该板块）
    let site_domain = site_domain_of(&state.config.site_url);
    let internal_links =
        analytics::internal_links(&state.db, from, to, tz, &site_domain, TOP_INTERNAL_LINKS)
            .await?;

    // 统计卡
    ctx.insert(
        "summary",
        &json!({
            "total_views": summary.total_views,
            "unique_visitors": summary.unique_visitors,
            "new_visitors": summary.new_visitors,
            "returning_visitors": summary.returning_visitors,
            "bot_views": summary.bot_views,
            "views_per_visitor": (summary.views_per_visitor * 10.0).round() / 10.0,
        }),
    );

    // 设备构成标签（key → 中文名）
    let devices = devices
        .iter()
        .map(|d| json!({ "key": d.key, "label": device_label(&d.key), "count": d.count }))
        .collect::<Vec<_>>();
    ctx.insert("devices", &devices);

    ctx.insert("browsers", &browsers);
    ctx.insert("os_list", &os);
    ctx.insert("domains", &domains);
    ctx.insert("paths", &paths);
    ctx.insert("keywords", &keywords);
    ctx.insert("internal_links", &internal_links);
    ctx.insert("post_sources", &post_sources);
    ctx.insert("recent", &recent);

    // 图表数据（内嵌 JSON 供 admin.js 画图，走 safe_string 避免 tera 转义）
    let chart_json = json!({
        "labels": daily.iter().map(|d| d.date.clone()).collect::<Vec<_>>(),
        "views": daily.iter().map(|d| d.views).collect::<Vec<_>>(),
        "visitors": daily.iter().map(|d| d.visitors).collect::<Vec<_>>(),
        "devices": devices,
        "browsers": browsers,
        "os": os,
        "heatmap": heatmap,
    })
    .to_string();
    ctx.insert_value("chart_data", tera::Value::safe_string(&chart_json));

    // 爬虫占比（爬虫 PV / 全部 PV），分母含爬虫
    let total_all = summary.total_views + summary.bot_views;
    let bot_ratio = if total_all > 0 {
        (summary.bot_views as f64 / total_all as f64 * 1000.0).round() / 10.0
    } else {
        0.0
    };
    ctx.insert("bot_ratio", &bot_ratio);
    // 有无站内互链数据（无 site_url 配置时整块隐藏）
    ctx.insert("has_internal_links", &!internal_links.is_empty());
    Ok(())
}

/// 从 `site_url`（如 `https://hancic.site/`）取域名，供站内互链判定。
fn site_domain_of(site_url: &str) -> String {
    let trimmed = site_url.trim().trim_end_matches('/');
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    // 去掉路径与端口
    let host = without_scheme.split('/').next().unwrap_or(without_scheme);
    host.split(':').next().unwrap_or(host).to_ascii_lowercase()
}

/// 设备类型 key → 中文名。
fn device_label(key: &str) -> &'static str {
    match key {
        "desktop" => "桌面端",
        "mobile" => "移动端",
        "tablet" => "平板",
        "bot" => "爬虫",
        _ => "其他",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_domain_extracted_from_url() {
        assert_eq!(site_domain_of("https://hancic.site/"), "hancic.site");
        assert_eq!(
            site_domain_of("http://blog.example.com/path"),
            "blog.example.com"
        );
        assert_eq!(site_domain_of("https://hancic.site:8080/x"), "hancic.site");
        assert_eq!(site_domain_of(""), "");
    }

    #[test]
    fn device_labels_cover_all_keys() {
        assert_eq!(device_label("desktop"), "桌面端");
        assert_eq!(device_label("mobile"), "移动端");
        assert_eq!(device_label("tablet"), "平板");
        assert_eq!(device_label("bot"), "爬虫");
        assert_eq!(device_label("whatever"), "其他");
    }
}
