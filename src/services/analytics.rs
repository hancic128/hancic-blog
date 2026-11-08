//! 访问分析服务：真实访客 / 设备构成 / 来源明细 / 搜索关键词 /
//! 时段热力 / 站内互链 / 单篇下钻 / 实时访问流。
//!
//! 与 `services::stats` 的分工：`stats` 负责「总量 + 趋势 + 排行 + 地区 +
//! 来源分类」，是仪表盘的摘要口径；本模块负责**维度下钻**，是「访问分析」
//! 页的明细口径。
//!
//! 两个模块共用同一套时间语义：`from`/`to` 为站点时区的自然日，
//! 边界换算见 `services::timezone::local_day_utc_bounds`。
//!
//! **爬虫口径**：真实阅读量要把爬虫剔除——搜索引擎爬虫每天抓取文章
//! 会带来大量 PV，混在一起会让「访问量」虚高数倍。本模块所有面向
//! 「读者」的指标默认排除 `device = 'bot'`，需要看原始 PV 时才显式包含。

use crate::db::Db;
use crate::error::AppResult;
use chrono::{DateTime, Datelike, NaiveDateTime, Timelike, Utc};
use chrono_tz::Tz;
use serde::Serialize;
use sqlx::Sqlite;
use sqlx::sqlite::{SqliteArguments, SqliteRow};

/// 分组计数行（维度名 + 计数）。
#[derive(Debug, Clone, Serialize)]
pub struct CountRow {
    pub key: String,
    pub count: i64,
}

/// 独立访客 / 页面浏览概览。
#[derive(Debug, Clone, Serialize)]
pub struct VisitorSummary {
    /// 总访问次数（PV，剔除爬虫）。
    pub total_views: i64,
    /// 独立访客数（UV，按访客指纹去重）。
    pub unique_visitors: i64,
    /// 新访客数（区间内首次出现的访客）。
    pub new_visitors: i64,
    /// 回访访客数（区间内出现过、且此前已访问过）。
    pub returning_visitors: i64,
    /// 爬虫访问次数。
    pub bot_views: i64,
    /// 人均阅读篇数（PV / UV）。
    pub views_per_visitor: f64,
}

/// 每日独立访客与访问量。
#[derive(Debug, Clone, Serialize)]
pub struct DailyVisitors {
    pub date: String,
    pub views: i64,
    pub visitors: i64,
}

/// 搜索关键词行。
#[derive(Debug, Clone, Serialize)]
pub struct KeywordRow {
    pub keyword: String,
    pub count: i64,
    /// 命中该关键词的文章（去重）。
    pub post_count: i64,
}

/// 站内互链行：从站内某页跳到站内某页。
#[derive(Debug, Clone, Serialize)]
pub struct InternalLinkRow {
    /// 来源页面路径（referer 的 path）。
    pub from_path: String,
    /// 落地文章 id。
    pub to_post_id: i64,
    /// 落地文章标题。
    pub to_title: String,
    pub count: i64,
}

/// 单篇文章的来源构成。
#[derive(Debug, Clone, Serialize)]
pub struct PostSourceRow {
    pub post_id: i64,
    pub title: String,
    pub source: String,
    pub label: String,
    pub count: i64,
}

/// 最近访问流的一行。
#[derive(Debug, Clone, Serialize)]
pub struct RecentView {
    pub post_id: i64,
    pub post_title: String,
    pub device: String,
    pub browser: String,
    pub os: String,
    pub source: String,
    pub country: String,
    pub province: String,
    /// 相对时间描述（如「3 分钟前」）。
    pub ago: String,
    /// 访问时刻（站点时区 `YYYY-MM-DD HH:MM`）。
    pub at: String,
}

/// 时段热力：星期几 × 小时。
#[derive(Debug, Clone, Serialize)]
pub struct HeatmapCell {
    /// 0 = 周一 … 6 = 周日。
    pub weekday: i64,
    /// 0–23。
    pub hour: i64,
    pub count: i64,
}

/// 访问分析总览：一次性返回各维度，供后台页与 API 共用。
#[derive(Debug, Clone, Serialize)]
pub struct AnalyticsReport {
    pub summary: VisitorSummary,
    pub daily: Vec<DailyVisitors>,
    pub devices: Vec<CountRow>,
    pub browsers: Vec<CountRow>,
    pub os: Vec<CountRow>,
    pub referer_domains: Vec<CountRow>,
    pub referer_paths: Vec<CountRow>,
    pub keywords: Vec<KeywordRow>,
    pub heatmap: Vec<HeatmapCell>,
    pub internal_links: Vec<InternalLinkRow>,
    pub post_sources: Vec<PostSourceRow>,
    pub recent: Vec<RecentView>,
}

/// 时间范围 + 爬虫开关，编译成 SQL 条件片段。
///
/// 条件与绑定值分开返回（而非拼好的 WHERE 串），这样同一组条件可以
/// 复用多个查询（如「按文章聚合来源」时要额外加 `post_id = ?`，
/// 若条件已含 WHERE 前缀就会拼出 `WHERE post_id = ? WHERE ...` 的非法 SQL）。
struct Filter {
    conds: Vec<String>,
    /// 绑定值。SQLite 亲和性会把字符串数字转成整数，但显式区分类型更稳。
    binds: Vec<BindValue>,
}

/// 绑定值的类型标签。
#[derive(Debug, Clone)]
enum BindValue {
    Text(String),
    Int(i64),
}

impl Filter {
    /// 构造过滤条件：`from`/`to` 为站点时区自然日；`include_bots` 为真时
    /// 不排除爬虫（用于统计爬虫 PV）。
    fn new(from: Option<&str>, to: Option<&str>, tz: &Tz, include_bots: bool) -> Self {
        Self::with_col("created_at", from, to, tz, include_bots)
    }

    /// 同 `new`，但指定时间列名。
    ///
    /// JOIN `posts` 的查询里 `created_at` 同时存在于 `page_views` 与 `posts`
    /// 两张表，不加表前缀会触发 SQLite 的「ambiguous column name」错误。
    fn with_col(
        col: &str,
        from: Option<&str>,
        to: Option<&str>,
        tz: &Tz,
        include_bots: bool,
    ) -> Self {
        let (lower, upper) = crate::services::timezone::local_day_utc_bounds(from, to, tz);
        let mut conds: Vec<String> = Vec::new();
        let mut binds: Vec<BindValue> = Vec::new();
        if let Some(f) = lower {
            conds.push(format!("{col} >= ?"));
            binds.push(BindValue::Text(f));
        }
        if let Some(t) = upper {
            conds.push(format!("{col} < ?"));
            binds.push(BindValue::Text(t));
        }
        if !include_bots {
            conds.push("device != 'bot'".to_string());
        }
        Self { conds, binds }
    }

    /// 追加一个等值条件。
    fn eq(mut self, col: &str, value: &str) -> Self {
        self.conds.push(format!("{col} = ?"));
        self.binds.push(BindValue::Text(value.to_string()));
        self
    }

    /// 追加一个整数等值条件。
    fn eq_int(mut self, col: &str, value: i64) -> Self {
        self.conds.push(format!("{col} = ?"));
        self.binds.push(BindValue::Int(value));
        self
    }

    /// 渲染为 WHERE 子句（无条件时返回空串）。
    fn where_sql(&self) -> String {
        if self.conds.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", self.conds.join(" AND "))
        }
    }
}

/// 把绑定值依次绑到查询上。
///
/// `sqlx` 的 `QueryAs` 在绑定后生命周期与绑定值绑定，故 `Filter` 必须
/// 活到查询执行结束——签名里 `filter` 与 `q` 同生命周期。
fn bind_filter<'a, O>(
    mut q: sqlx::query::QueryAs<'a, Sqlite, O, SqliteArguments<'a>>,
    filter: &'a Filter,
) -> sqlx::query::QueryAs<'a, Sqlite, O, SqliteArguments<'a>>
where
    O: for<'r> sqlx::FromRow<'r, SqliteRow>,
{
    for b in &filter.binds {
        q = match b {
            BindValue::Text(v) => q.bind(v),
            BindValue::Int(v) => q.bind(v),
        };
    }
    q
}

/// 分组计数查询：`SELECT <expr> AS k, COUNT(*) AS c FROM page_views ... GROUP BY k ORDER BY c DESC`。
async fn grouped_counts(
    db: &Db,
    expr: &str,
    from: Option<&str>,
    to: Option<&str>,
    tz: &Tz,
    limit: i64,
) -> AppResult<Vec<CountRow>> {
    let filter = Filter::new(from, to, tz, false);
    let sql = format!(
        "SELECT {expr} AS k, COUNT(*) AS c FROM page_views {}
         GROUP BY k ORDER BY c DESC, k ASC LIMIT ?",
        filter.where_sql()
    );
    let rows = bind_filter(sqlx::query_as::<_, (String, i64)>(&sql), &filter)
        .bind(limit)
        .fetch_all(db)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(key, count)| CountRow { key, count })
        .collect())
}

/// 访问概览：PV / UV / 新客 / 回访 / 爬虫占比。
///
/// 新老访客判定：一个访客若**全局首次访问**落在区间内则为新客，
/// 否则为回客。全局首次访问时间取自全表（不看区间），因此区间起点之前
/// 就来过的访客计为回客——这正是新老划分的本意。
pub async fn visitor_summary(
    db: &Db,
    from: Option<&str>,
    to: Option<&str>,
    tz: &Tz,
) -> AppResult<VisitorSummary> {
    let filter = Filter::new(from, to, tz, false);
    // 区间内总 PV
    let total_views: i64 = bind_filter(
        sqlx::query_as::<_, (i64,)>(&format!(
            "SELECT COUNT(*) FROM page_views {}",
            filter.where_sql()
        )),
        &filter,
    )
    .fetch_one(db)
    .await?
    .0;
    // 区间内独立访客（访客指纹去重；与「真实阅读」同口径，不计爬虫）
    let unique_visitors: i64 = bind_filter(
        sqlx::query_as::<_, (i64,)>(&format!(
            "SELECT COUNT(DISTINCT visitor_id) FROM page_views {} AND visitor_id != ''",
            filter.where_sql()
        )),
        &filter,
    )
    .fetch_one(db)
    .await?
    .0;
    // 新客 / 回客：取「区间内出现过的访客」及其**全局首访时间**，
    // 首访时间落在区间内 → 新客，早于区间 → 回客。
    //
    // 边界用区间的下界（`from` 对应的 UTC 时刻），而非区间内最早的那条
    // 记录——后者依赖数据分布，区间前半段没有访问时会得出「那天之后来的人
    // 算新客」这种反直觉结论。
    let (lower_bound, _) = crate::services::timezone::local_day_utc_bounds(from, to, tz);
    let new_returning_sql = format!(
        "WITH first_seen AS (
             SELECT visitor_id, MIN(created_at) AS first_at
             FROM page_views WHERE visitor_id != '' GROUP BY visitor_id
         ),
         in_range AS (
             SELECT DISTINCT visitor_id FROM page_views {} AND visitor_id != ''
         )
         SELECT f.visitor_id, f.first_at
         FROM first_seen f JOIN in_range i ON i.visitor_id = f.visitor_id",
        filter.where_sql()
    );
    let rows = bind_filter(
        sqlx::query_as::<_, (String, String)>(&new_returning_sql),
        &filter,
    )
    .fetch_all(db)
    .await?;
    let mut new_visitors = 0i64;
    let mut returning_visitors = 0i64;
    for (_visitor, first_at) in rows {
        // 无下界（「全部」区间）时所有访客的首访都算在区间内 → 全是新客
        let is_new = match &lower_bound {
            Some(bound) => first_at.as_str() >= bound.as_str(),
            None => true,
        };
        if is_new {
            new_visitors += 1;
        } else {
            returning_visitors += 1;
        }
    }
    // 爬虫 PV（不套 device 过滤）
    let bot_filter = Filter::new(from, to, tz, true).eq("device", "bot");
    let bot_views: i64 = bind_filter(
        sqlx::query_as::<_, (i64,)>(&format!(
            "SELECT COUNT(*) FROM page_views {}",
            bot_filter.where_sql()
        )),
        &bot_filter,
    )
    .fetch_one(db)
    .await?
    .0;
    let views_per_visitor = if unique_visitors > 0 {
        total_views as f64 / unique_visitors as f64
    } else {
        0.0
    };
    Ok(VisitorSummary {
        total_views,
        unique_visitors,
        new_visitors,
        returning_visitors,
        bot_views,
        views_per_visitor,
    })
}

/// 每日 PV 与 UV 趋势。
pub async fn daily_visitors(
    db: &Db,
    from: Option<&str>,
    to: Option<&str>,
    tz: &Tz,
) -> AppResult<Vec<DailyVisitors>> {
    let filter = Filter::new(from, to, tz, false);
    let sql = format!(
        "SELECT created_at, visitor_id FROM page_views {} ORDER BY created_at",
        filter.where_sql()
    );
    let rows = bind_filter(sqlx::query_as::<_, (String, String)>(&sql), &filter)
        .fetch_all(db)
        .await?;
    // 按站点时区自然日聚合：PV 逐条计数，UV 按当日去重访客
    let mut by_day: Vec<(String, i64, Vec<String>)> = Vec::new();
    for (raw, visitor) in rows {
        let Some(day) = parse_day(&raw, tz) else {
            continue;
        };
        match by_day.last_mut() {
            Some((d, count, visitors)) if *d == day => {
                *count += 1;
                if !visitor.is_empty() && !visitors.contains(&visitor) {
                    visitors.push(visitor);
                }
            }
            _ => {
                let mut visitors = Vec::new();
                if !visitor.is_empty() {
                    visitors.push(visitor);
                }
                by_day.push((day, 1, visitors));
            }
        }
    }
    Ok(by_day
        .into_iter()
        .map(|(date, views, visitors)| DailyVisitors {
            date,
            views,
            visitors: visitors.len() as i64,
        })
        .collect())
}

/// 把 UTC 时间串解析成站点时区下的自然日。
fn parse_day(raw: &str, tz: &Tz) -> Option<String> {
    let ndt = NaiveDateTime::parse_from_str(raw.trim_end_matches('Z'), "%Y-%m-%dT%H:%M:%S").ok()?;
    let utc_dt = DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc);
    Some(utc_dt.with_timezone(tz).format("%Y-%m-%d").to_string())
}

/// 设备构成（桌面/移动/平板/其他；爬虫已排除）。
pub async fn by_device(
    db: &Db,
    from: Option<&str>,
    to: Option<&str>,
    tz: &Tz,
) -> AppResult<Vec<CountRow>> {
    grouped_counts(db, "device", from, to, tz, 10).await
}

/// 浏览器构成。
pub async fn by_browser(
    db: &Db,
    from: Option<&str>,
    to: Option<&str>,
    tz: &Tz,
) -> AppResult<Vec<CountRow>> {
    grouped_counts(db, "browser", from, to, tz, 15).await
}

/// 操作系统构成。
pub async fn by_os(
    db: &Db,
    from: Option<&str>,
    to: Option<&str>,
    tz: &Tz,
) -> AppResult<Vec<CountRow>> {
    grouped_counts(db, "os", from, to, tz, 15).await
}

/// 来源域名明细（完整域名，不做平台归类）。
///
/// 直接访问（无 referer）记为空串，模板层显示为「直接访问」。
pub async fn by_referer_domain(
    db: &Db,
    from: Option<&str>,
    to: Option<&str>,
    tz: &Tz,
    limit: i64,
) -> AppResult<Vec<CountRow>> {
    grouped_counts(db, "ref_domain", from, to, tz, limit).await
}

/// 来源路径明细（对方站点的具体页面）。
pub async fn by_referer_path(
    db: &Db,
    from: Option<&str>,
    to: Option<&str>,
    tz: &Tz,
    limit: i64,
) -> AppResult<Vec<CountRow>> {
    grouped_counts(db, "ref_path", from, to, tz, limit).await
}

/// 搜索关键词（读者在搜什么）。
///
/// 只取搜索引擎来源且抽到了关键词的行；按关键词聚合，附带命中文章数。
pub async fn search_keywords(
    db: &Db,
    from: Option<&str>,
    to: Option<&str>,
    tz: &Tz,
    limit: i64,
) -> AppResult<Vec<KeywordRow>> {
    let filter = Filter::new(from, to, tz, false);
    let sql = format!(
        "SELECT search_query, COUNT(*) AS c, COUNT(DISTINCT post_id) AS pc
         FROM page_views {} AND search_query != ''
         GROUP BY search_query ORDER BY c DESC, search_query ASC LIMIT ?",
        filter.where_sql()
    );
    let rows = bind_filter(sqlx::query_as::<_, (String, i64, i64)>(&sql), &filter)
        .bind(limit)
        .fetch_all(db)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(keyword, count, post_count)| KeywordRow {
            keyword,
            count,
            post_count,
        })
        .collect())
}

/// 时段热力：星期几 × 小时（站点时区）。
///
/// 返回 7×24 = 168 格（计数为 0 的格子也补齐），前端可直接铺网格。
pub async fn hourly_heatmap(
    db: &Db,
    from: Option<&str>,
    to: Option<&str>,
    tz: &Tz,
) -> AppResult<Vec<HeatmapCell>> {
    let filter = Filter::new(from, to, tz, false);
    let sql = format!("SELECT created_at FROM page_views {}", filter.where_sql());
    let rows = bind_filter(sqlx::query_as::<_, (String,)>(&sql), &filter)
        .fetch_all(db)
        .await?;
    // 7×24 计数矩阵，周一为一周之首（与中文习惯一致）
    let mut grid = [[0i64; 24]; 7];
    for (raw,) in rows {
        let Ok(ndt) = NaiveDateTime::parse_from_str(raw.trim_end_matches('Z'), "%Y-%m-%dT%H:%M:%S")
        else {
            continue;
        };
        let utc_dt = DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc);
        let local = utc_dt.with_timezone(tz);
        let hour = local.hour() as usize;
        // chrono 的 weekday：周一 = 0 … 周日 = 6
        let weekday = local.weekday().num_days_from_monday() as usize;
        grid[weekday][hour] += 1;
    }
    let mut out = Vec::with_capacity(168);
    for (weekday, row) in grid.iter().enumerate() {
        for (hour, count) in row.iter().enumerate() {
            out.push(HeatmapCell {
                weekday: weekday as i64,
                hour: hour as i64,
                count: *count,
            });
        }
    }
    Ok(out)
}

/// 站内互链：哪些站内页面引荐了哪些文章。
///
/// 判定依据是 Referer 域名指向本站：`ref_domain` 等于本站域名、`www.` 前缀
/// 形式，或本站的任意子域。`site_domain` 为空时返回空列表（无法判定站内）。
///
/// **域名过滤必须在 SQL 里做**：若先取 Top N 再在应用层过滤，直接访问
/// （`ref_domain` 为空，量最大）会占满名额，导致站内链接一条都取不到。
/// 子域判定（`blog.` 前缀、`www.` 前缀）无法用单个 `LIKE` 覆盖，故对
/// 「主域 + 子域」两种形态分别取数后合并。
pub async fn internal_links(
    db: &Db,
    from: Option<&str>,
    to: Option<&str>,
    tz: &Tz,
    site_domain: &str,
    limit: i64,
) -> AppResult<Vec<InternalLinkRow>> {
    let site_domain = site_domain.trim();
    if site_domain.is_empty() {
        return Ok(Vec::new());
    }
    let filter = Filter::with_col("pv.created_at", from, to, tz, false);
    // 站内域名形态：主域本身、`www.` 前缀、以及任意子域
    let sql = format!(
        "SELECT pv.ref_path AS from_path, p.id AS post_id, p.title AS title, COUNT(*) AS c
         FROM page_views pv JOIN posts p ON p.id = pv.post_id
         {}
         AND (pv.ref_domain = ?
              OR pv.ref_domain = ?
              OR pv.ref_domain LIKE ?)
         GROUP BY pv.ref_path, p.id ORDER BY c DESC LIMIT ?",
        filter.where_sql()
    );
    let rows = bind_filter(
        sqlx::query_as::<_, (String, i64, String, i64)>(&sql),
        &filter,
    )
    .bind(site_domain)
    .bind(format!("www.{site_domain}"))
    .bind(format!("%.{site_domain}"))
    .bind(limit)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(from_path, post_id, to_title, count)| InternalLinkRow {
            from_path,
            to_post_id: post_id,
            to_title,
            count,
        })
        .collect())
}

/// 单篇文章的来源构成（每篇文章各自带来多少流量、从哪来）。
///
/// 取区间内阅读最多的 `top_posts` 篇文章，逐篇列出其来源分类计数——
/// 回答「这篇文章的流量是谁带来的」这个仪表盘回答不了的问题。
pub async fn post_source_breakdown(
    db: &Db,
    from: Option<&str>,
    to: Option<&str>,
    tz: &Tz,
    top_posts: i64,
    limit: i64,
) -> AppResult<Vec<PostSourceRow>> {
    let filter = Filter::with_col("pv.created_at", from, to, tz, false);
    // 先取区间内阅读最多的文章
    let top_sql = format!(
        "SELECT pv.post_id AS post_id, p.title AS title, COUNT(*) AS c
         FROM page_views pv JOIN posts p ON p.id = pv.post_id
         {}
         GROUP BY pv.post_id ORDER BY c DESC LIMIT ?",
        filter.where_sql()
    );
    let top_rows = bind_filter(sqlx::query_as::<_, (i64, String, i64)>(&top_sql), &filter)
        .bind(top_posts)
        .fetch_all(db)
        .await?;
    let mut out: Vec<PostSourceRow> = Vec::new();
    for (post_id, title, _) in top_rows {
        // 该文章的来源分布（同一组时间/爬虫条件 + post_id 限定）
        let src_filter = Filter::new(from, to, tz, false).eq_int("post_id", post_id);
        let src_sql = format!(
            "SELECT source, COUNT(*) AS c FROM page_views {}
             GROUP BY source ORDER BY c DESC LIMIT ?",
            src_filter.where_sql()
        );
        let src_rows = bind_filter(sqlx::query_as::<_, (String, i64)>(&src_sql), &src_filter)
            .bind(limit)
            .fetch_all(db)
            .await?;
        for (source, count) in src_rows {
            out.push(PostSourceRow {
                post_id,
                title: title.clone(),
                label: crate::admin::stats::source_label(&source).to_string(),
                source,
                count,
            });
        }
    }
    Ok(out)
}

/// 最近访问流（最新 N 条）。
///
/// 时间倒序，按站点时区格式化；`ago` 给出「几分钟前 / 小时前 / 天前」。
pub async fn recent_views(db: &Db, site_tz: &Tz, limit: i64) -> AppResult<Vec<RecentView>> {
    let sql = "SELECT pv.post_id AS post_id, COALESCE(p.title, '（文章已删除）') AS title,
                      pv.device AS device, pv.browser AS browser, pv.os AS os,
                      pv.source AS source, pv.country AS country, pv.province AS province,
                      pv.created_at AS created_at
               FROM page_views pv LEFT JOIN posts p ON p.id = pv.post_id
               ORDER BY pv.created_at DESC, pv.id DESC LIMIT ?";
    let rows = sqlx::query_as::<
        _,
        (
            i64,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
        ),
    >(sql)
    .bind(limit)
    .fetch_all(db)
    .await?;
    let now = Utc::now();
    Ok(rows
        .into_iter()
        .map(
            |(post_id, title, device, browser, os, source, country, province, created_at)| {
                let ndt = NaiveDateTime::parse_from_str(
                    created_at.trim_end_matches('Z'),
                    "%Y-%m-%dT%H:%M:%S",
                );
                let (at, ago) = match ndt {
                    Ok(ndt) => {
                        let utc_dt = DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc);
                        let at = utc_dt
                            .with_timezone(site_tz)
                            .format("%Y-%m-%d %H:%M")
                            .to_string();
                        let ago = humanize_ago(now - utc_dt);
                        (at, ago)
                    }
                    Err(_) => (created_at.clone(), "—".to_string()),
                };
                RecentView {
                    post_id,
                    post_title: title,
                    device,
                    browser,
                    os,
                    source,
                    country,
                    province,
                    ago,
                    at,
                }
            },
        )
        .collect())
}

/// 把时间差转成「N 分钟前 / N 小时前 / N 天前」。
fn humanize_ago(delta: chrono::Duration) -> String {
    let secs = delta.num_seconds().max(0);
    if secs < 60 {
        format!("{secs} 秒前")
    } else if secs < 3600 {
        format!("{} 分钟前", secs / 60)
    } else if secs < 86400 {
        format!("{} 小时前", secs / 3600)
    } else {
        format!("{} 天前", secs / 86400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humanize_ago_ranges() {
        assert_eq!(humanize_ago(chrono::Duration::seconds(30)), "30 秒前");
        assert_eq!(humanize_ago(chrono::Duration::minutes(5)), "5 分钟前");
        assert_eq!(humanize_ago(chrono::Duration::hours(3)), "3 小时前");
        assert_eq!(humanize_ago(chrono::Duration::days(2)), "2 天前");
    }

    #[test]
    fn parse_day_converts_to_site_timezone() {
        let tz = Tz::Asia__Shanghai;
        // UTC 2026-01-01T17:00Z → 上海 2026-01-02 01:00
        let day = parse_day("2026-01-01T17:00:00Z", &tz).unwrap();
        assert_eq!(day, "2026-01-02");
    }

    #[test]
    fn parse_day_rejects_malformed() {
        assert!(parse_day("not-a-date", &Tz::Asia__Shanghai).is_none());
    }

    #[test]
    fn filter_renders_where_clause() {
        let f = Filter::new(None, None, &Tz::Asia__Shanghai, false);
        // 无时间条件时仍排除爬虫
        assert_eq!(f.where_sql(), "WHERE device != 'bot'");
        assert!(f.binds.is_empty());
    }

    #[test]
    fn filter_binds_date_bounds() {
        let f = Filter::new(
            Some("2026-01-01"),
            Some("2026-01-31"),
            &Tz::Asia__Shanghai,
            false,
        );
        assert_eq!(f.binds.len(), 2);
        assert!(f.where_sql().contains("created_at >= ?"));
        assert!(f.where_sql().contains("created_at < ?"));
    }
}
