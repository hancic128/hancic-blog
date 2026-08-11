//! 后台统计：总览卡片 + 趋势折线（Chart.js）+ 文章排行 + 地区下钻。
//!
//! 路由：
//!   GET  /admin/stats?from=&to=                        总览（卡片 + 趋势 + 排行 + 地区国家层）
//!   GET  /admin/stats/posts?from=&to=&page=            文章排行（期间浏览量降序，分页）
//!   GET  /admin/stats/regions?from=&to=&country=&province=  地区国家→省→市下钻
//!   POST /admin/stats/clear                            清空阅读明细日志（前端二次确认）
//!
//! from/to 为 `YYYY-MM-DD`，缺省近 30 天（含今天）；非法 → 400 提示。
//! 三张 GET 页共用 stats.html：差异仅在下钻筛选（country/province）与排行分页
//! （`sub` 高亮当前子页）。
//!
//! M51（T12 遗留）：趋势按 UTC 日期分组，与仪表盘趋势保持一致；模板已注明
//! 「UTC 日期分组」。统一到 Asia/Shanghai 需要改服务层聚合 SQL 与仪表盘横轴，
//! 超出本任务文件范围，留待后续任务处理。
//!
//! 鉴权约定同其他后台模块：GET 未登录 302 跳登录；POST 先 `require_admin`
//! 再过 CSRF。

use crate::error::AppError;
use crate::services::stats;
use crate::{session, AppState};
use axum::extract::{Form, OriginalUri, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use chrono::{Days, NaiveDate, Utc};
use serde_json::{Value, json};
use std::collections::HashMap;
use tower_sessions::Session;

/// 排行每页条数。
const POSTS_PAGE_SIZE: usize = 20;
/// 排行一次性拉取上限（服务层 `top_posts` 无 offset，个人博客规模内足够）。
const TOP_POSTS_CAP: i64 = 1000;

// ---------- 总览 ----------

pub async fn index(
    State(state): State<AppState>,
    session: Session,
    Query(query): Query<HashMap<String, String>>,
    uri: OriginalUri,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect(&state.config.base_path, "/admin/login");
    }
    let (from, to) = match parse_range(&query) {
        Ok(v) => v,
        Err(msg) => return bad_request(&msg),
    };
    render_or_500(
        render(
            &state,
            &session,
            uri.path(),
            &from,
            &to,
            "overview",
            None,
            None,
            1,
            query_days(&query),
        )
        .await,
    )
}

// ---------- 文章排行 ----------

pub async fn ranking(
    State(state): State<AppState>,
    session: Session,
    Query(query): Query<HashMap<String, String>>,
    uri: OriginalUri,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect(&state.config.base_path, "/admin/login");
    }
    let (from, to) = match parse_range(&query) {
        Ok(v) => v,
        Err(msg) => return bad_request(&msg),
    };
    let page = query
        .get("page")
        .and_then(|p| p.parse::<usize>().ok())
        .filter(|&p| p > 0)
        .unwrap_or(1);
    render_or_500(
        render(
            &state,
            &session,
            uri.path(),
            &from,
            &to,
            "posts",
            None,
            None,
            page,
            query_days(&query),
        )
        .await,
    )
}

// ---------- 地区下钻 ----------

pub async fn regions(
    State(state): State<AppState>,
    session: Session,
    Query(query): Query<HashMap<String, String>>,
    uri: OriginalUri,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect(&state.config.base_path, "/admin/login");
    }
    let (from, to) = match parse_range(&query) {
        Ok(v) => v,
        Err(msg) => return bad_request(&msg),
    };
    let country = query
        .get("country")
        .map(String::as_str)
        .filter(|s| !s.is_empty());
    let province = query
        .get("province")
        .map(String::as_str)
        .filter(|s| !s.is_empty());
    render_or_500(
        render(
            &state,
            &session,
            uri.path(),
            &from,
            &to,
            "regions",
            country,
            province,
            1,
            query_days(&query),
        )
        .await,
    )
}

// ---------- 清理日志 ----------

pub async fn clear(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<HashMap<String, String>>,
) -> Result<Response, AppError> {
    session::require_admin(&session).await?;
    session::verify_csrf(&session, form.get("csrf").map(String::as_str)).await?;
    match stats::clear_logs(&state.db).await {
        Ok(()) => Ok(super::redirect(&state.config.base_path, "/admin/stats")),
        Err(e) => {
            tracing::error!("清理阅读日志失败: {e:?}");
            Ok(super::redirect(&state.config.base_path, "/admin/stats"))
        }
    }
}

// ---------- 渲染 ----------

/// 渲染统计页。三张 GET 页共用：卡片 + 趋势 + 排行 + 地区表，
/// `sub` 控制子导航高亮，`country`/`province` 控制地区下钻层级，`page` 控制排行分页。
#[allow(clippy::too_many_arguments)]
async fn render(
    state: &AppState,
    session: &Session,
    path: &str,
    from: &Option<String>,
    to: &Option<String>,
    sub: &str,
    country: Option<&str>,
    province: Option<&str>,
    page: usize,
    days: u32,
) -> Result<Response, AppError> {
    let (mut ctx, _csrf) = super::base_ctx(state, session, path).await;

    // 有效区间（缺省一侧补近 30 天）：表单回填 + 趋势横轴都用它
    let (from_str, to_str) = effective_range(from, to);
    ctx.insert("from", &from_str);
    ctx.insert("to", &to_str);
    ctx.insert("sub", sub);
    ctx.insert("days", &days);

    // 总览卡片（total_views 随区间过滤，文章/说说/附件为全量）
    let summary = stats::summary(&state.db, from.as_deref(), to.as_deref()).await?;
    ctx.insert(
        "stats",
        &json!({
            "total_views": summary.total_views,
            "total_posts": summary.total_posts,
            "total_moments": summary.total_moments,
            "total_attachments": summary.total_attachments,
        }),
    );

    // 趋势：横轴覆盖有效区间，无阅读的日期补 0；内嵌 JSON 供 admin.js 画图
    // （insert_value 保留 safe 标记，避免 tera 转义破坏脚本，与仪表盘一致）。
    let days = date_range(&from_str, &to_str);
    let counts: HashMap<&str, i64> = summary
        .trend
        .iter()
        .map(|d| (d.date.as_str(), d.count))
        .collect();
    let trend_data: Vec<i64> = days
        .iter()
        .map(|d| counts.get(d.as_str()).copied().unwrap_or(0))
        .collect();
    let chart_json = json!({ "labels": days, "data": trend_data }).to_string();
    ctx.insert_value("chart_data", tera::Value::safe_string(&chart_json));

    // 排行：服务层一次拉取（上限内），内存分页
    let top = stats::top_posts(&state.db, from.as_deref(), to.as_deref(), TOP_POSTS_CAP).await?;
    let total = top.len();
    let total_pages = total.div_ceil(POSTS_PAGE_SIZE).max(1);
    let page = page.min(total_pages);
    let slice = top
        .iter()
        .skip((page - 1) * POSTS_PAGE_SIZE)
        .take(POSTS_PAGE_SIZE);
    ctx.insert(
        "posts",
        &slice
            .map(|(p, period)| {
                json!({
                    "id": p.id,
                    "title": p.title,
                    "views": p.views,
                    "period_views": period,
                })
            })
            .collect::<Vec<_>>(),
    );
    ctx.insert("post_total", &total);
    ctx.insert("post_page", &page);
    ctx.insert("post_total_pages", &total_pages);
    ctx.insert("post_page_size", &POSTS_PAGE_SIZE);

    // 地区：按 country/province 折叠到当前下钻层级
    let regions = stats::by_region(&state.db, from.as_deref(), to.as_deref()).await?;
    let (region_rows, dim, back_url, back_label) =
        region_view(&regions, country, province, &from_str, &to_str);
    ctx.insert("regions", &region_rows);
    ctx.insert("region_dim", dim);
    ctx.insert("region_country", country.unwrap_or(""));
    ctx.insert("region_province", province.unwrap_or(""));
    if let Some(url) = back_url {
        ctx.insert("region_back_url", &url);
        ctx.insert("region_back_label", back_label);
    }

    Ok(super::render_admin(state, "stats.html", &ctx))
}

/// 查询失败兜底：记日志并返回 500（页面数据依赖库查询，出错不渲染假数据）。
fn render_or_500(res: Result<Response, AppError>) -> Response {
    match res {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("统计页查询失败: {e:?}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html("统计查询失败，请稍后重试"),
            )
                .into_response()
        }
    }
}

/// 非法 from/to：400 提示。
fn bad_request(msg: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Html(format!("<p>400 参数错误：{msg}</p>")),
    )
        .into_response()
}

// ---------- 参数与数据加工 ----------

/// 快捷天数参数：`days=30/60/90`（默认 30，上限 3650），供模板快捷按钮回显。
fn query_days(query: &HashMap<String, String>) -> u32 {
    query
        .get("days")
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(30)
        .clamp(1, 3650)
}

/// 解析 from/to（`YYYY-MM-DD`）：两者缺省 → 近 30 天（含今天）；
/// 支持 `days=30/60/90` 快捷参数（仅当 from/to 都缺省时生效，默认 30）；
/// 提供但非法 → Err；只给一侧时另一侧保持 None（服务层视为不设限）。
fn parse_range(
    query: &HashMap<String, String>,
) -> Result<(Option<String>, Option<String>), String> {
    let from = query
        .get("from")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let to = query
        .get("to")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    for (name, v) in [("from", &from), ("to", &to)] {
        if let Some(v) = v {
            if NaiveDate::parse_from_str(v, "%Y-%m-%d").is_err() {
                return Err(format!("{name} 参数必须是 YYYY-MM-DD"));
            }
        }
    }
    if let (Some(f), Some(t)) = (&from, &to) {
        let f = NaiveDate::parse_from_str(f, "%Y-%m-%d").expect("已校验");
        let t = NaiveDate::parse_from_str(t, "%Y-%m-%d").expect("已校验");
        if f > t {
            return Err("from 不能晚于 to".into());
        }
    }
    if from.is_none() && to.is_none() {
        // 快捷天数：days=30/60/90（默认 30），与「近 30 天」缺省行为一致
        let days = query
            .get("days")
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(30)
            .clamp(1, 3650);
        let today = Utc::now().date_naive();
        let from_d = today
            .checked_sub_days(Days::new(u64::from(days - 1)))
            .expect("days 上限内不会下溢");
        return Ok((
            Some(from_d.format("%Y-%m-%d").to_string()),
            Some(today.format("%Y-%m-%d").to_string()),
        ));
    }
    Ok((from, to))
}

/// 有效区间（缺省一侧补近 30 天），用于表单回填与趋势横轴。
fn effective_range(from: &Option<String>, to: &Option<String>) -> (String, String) {
    let today = Utc::now().date_naive();
    let from_str = from.clone().unwrap_or_else(|| {
        today
            .checked_sub_days(Days::new(29))
            .expect("30 天内日期不会下溢")
            .format("%Y-%m-%d")
            .to_string()
    });
    let to_str = to.clone().unwrap_or_else(|| today.format("%Y-%m-%d").to_string());
    (from_str, to_str)
}

/// `from` 到 `to`（含端点）的日期序列，升序；两端均已校验为 `YYYY-MM-DD`。
fn date_range(from: &str, to: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut d = NaiveDate::parse_from_str(from, "%Y-%m-%d").expect("已校验");
    let end = NaiveDate::parse_from_str(to, "%Y-%m-%d").expect("已校验");
    while d <= end {
        out.push(d.format("%Y-%m-%d").to_string());
        d = d.succ_opt().expect("日期递增不会溢出");
    }
    out
}

/// 地区下钻折叠：无 country → 国家层；有 country 无 province → 省份层；
/// 两级都有 → 城市层。行内 `drill` 为下钻链接（叶子层为空字符串）。
/// 空维度名保持原样，由模板显示为「未知」。
fn region_view(
    rows: &[stats::RegionStat],
    country: Option<&str>,
    province: Option<&str>,
    from: &str,
    to: &str,
) -> (Vec<Value>, &'static str, Option<String>, &'static str) {
    let (level, dim) = match (country, province) {
        (Some(_), Some(_)) => (2, "城市"),
        (Some(_), None) => (1, "省份"),
        _ => (0, "国家"),
    };
    let mut groups: Vec<(String, i64)> = Vec::new();
    for r in rows {
        let key = match level {
            0 => r.country.clone(),
            1 => {
                if r.country != country.unwrap_or_default() {
                    continue;
                }
                r.province.clone()
            }
            _ => {
                if r.country != country.unwrap_or_default()
                    || r.province != province.unwrap_or_default()
                {
                    continue;
                }
                r.city.clone()
            }
        };
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, c)) => *c += r.count,
            None => groups.push((key, r.count)),
        }
    }
    // 浏览量降序，同名升序
    groups.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let enc_country = country.map(urlencode).unwrap_or_default();
    let rows_value = groups
        .iter()
        .map(|(name, count)| {
            let drill = match level {
                0 => Some(format!(
                    "/admin/stats/regions?from={from}&to={to}&country={}",
                    urlencode(name)
                )),
                1 => Some(format!(
                    "/admin/stats/regions?from={from}&to={to}&country={enc_country}&province={}",
                    urlencode(name)
                )),
                _ => None,
            };
            json!({ "name": name, "count": count, "drill": drill.unwrap_or_default() })
        })
        .collect();
    let (back_url, back_label) = match level {
        1 => (
            Some(format!("/admin/stats/regions?from={from}&to={to}")),
            "国家",
        ),
        2 => (
            Some(format!(
                "/admin/stats/regions?from={from}&to={to}&country={enc_country}"
            )),
            "省份",
        ),
        _ => (None, ""),
    };
    (rows_value, dim, back_url, back_label)
}

/// 极简百分号编码（仅保留 unreserved 字符），用于地区下钻链接的 query 参数
/// （国家/省市为中文，须编码后放入链接）。
fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}
