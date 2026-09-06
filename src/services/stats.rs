//! 阅读统计服务：page_views 写入（含 ip2region 地区）、汇总、排行与清理。

use crate::db::Db;
use crate::error::AppResult;
use crate::ipregion::{Region, Searcher};
use crate::models::{Post, PostStatus, PostType};
use chrono::{DateTime, NaiveDateTime, Utc};
use chrono_tz::Tz;
use serde::Serialize;
use sqlx::{FromRow, Row};
use std::net::IpAddr;
use std::str::FromStr;

/// 每日阅读量（date 为 `YYYY-MM-DD`，UTC）。
#[derive(Debug, Clone, Serialize)]
pub struct DailyCount {
    pub date: String,
    pub count: i64,
}

/// 阅读汇总：总量 + 每日趋势。
#[derive(Debug, Clone, Serialize)]
pub struct StatsSummary {
    pub total_views: i64,
    pub total_posts: i64,
    pub total_moments: i64,
    pub total_attachments: i64,
    pub trend: Vec<DailyCount>,
}

/// 地区聚合行（按 country/province/city 分组）。
#[derive(Debug, Clone)]
pub struct RegionStat {
    pub country: String,
    pub province: String,
    pub city: String,
    pub count: i64,
}

/// 跳转来源聚合行（按 source 分类分组）。
#[derive(Debug, Clone)]
pub struct SourceStat {
    pub source: String,
    pub count: i64,
}

/// 跳转来源分类：按 referer 域名识别平台。key 存库（`classify_referer` 返回值），
/// 中文名映射见 `admin::stats::source_view`；无 referer 记直接访问，未知域名归其他。
pub fn classify_referer(referer: &str) -> &'static str {
    let r = referer.to_ascii_lowercase();
    if r.is_empty() {
        return "direct";
    }
    if r.contains("mp.weixin.qq.com") || r.contains("weixin") {
        return "wechat";
    }
    if r.contains("zhihu.com") {
        return "zhihu";
    }
    if r.contains("csdn.net") {
        return "csdn";
    }
    if r.contains("juejin.cn") {
        return "juejin";
    }
    if r.contains("weibo.com") {
        return "weibo";
    }
    if r.contains("jianshu.com") {
        return "jianshu";
    }
    if r.contains("github.com") {
        return "github";
    }
    if r.contains("google.") {
        return "google";
    }
    if r.contains("bing.com") {
        return "bing";
    }
    if r.contains("baidu.com") {
        return "baidu";
    }
    "other"
}

/// 记录一次阅读：事务内写入 page_views（含解析地区）并累加 posts.views。
pub async fn record_view(
    db: &Db,
    post_id: i64,
    ip: &str,
    ua: &str,
    referer: &str,
    searcher: &Searcher,
) -> AppResult<()> {
    let region = match ip.parse::<IpAddr>() {
        Ok(ip) => searcher.lookup(&ip),
        Err(_) => Region::local(),
    };
    let mut tx = db.begin().await?;
    sqlx::query(
        "INSERT INTO page_views(post_id, ip, ua, referer, source, country, province, city)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(post_id)
    .bind(ip)
    .bind(ua)
    .bind(referer)
    .bind(classify_referer(referer))
    .bind(&region.country)
    .bind(&region.province)
    .bind(&region.city)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE posts SET views = views + 1 WHERE id = ?")
        .bind(post_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// 阅读汇总：`from`/`to` 为 `YYYY-MM-DD`（站点时区日期，当日边界，上界开区间），
/// `None` 不设限；趋势按站点时区自然日分组（TZ 由 `tz` 指定）。
pub async fn summary(
    db: &Db,
    from: Option<&str>,
    to: Option<&str>,
    tz: &Tz,
) -> AppResult<StatsSummary> {
    let (where_sql, binds) = range_filter("created_at", from, to, tz);
    let count_sql = format!("SELECT COUNT(*) FROM page_views {where_sql}");
    let mut q = sqlx::query(&count_sql);
    for b in &binds {
        q = q.bind(b);
    }
    let total_views: i64 = q.fetch_one(db).await?.get(0);
    let total_posts: i64 = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM posts")
        .fetch_one(db)
        .await?;
    let total_moments: i64 = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM moments")
        .fetch_one(db)
        .await?;
    let total_attachments: i64 =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM attachments")
            .fetch_one(db)
            .await?;
    // 趋势：取范围内 created_at 逐条按站点时区归属自然日（SQLite 无 IANA，应用层分组；
    // 个人博客量级逐行读取可接受）
    let trend_sql = format!(
        "SELECT created_at FROM page_views {where_sql} ORDER BY created_at"
    );
    let mut trend_q = sqlx::query(&trend_sql);
    for b in &binds {
        trend_q = trend_q.bind(b);
    }
    let trend_rows = trend_q.fetch_all(db).await?;
    let mut by_day: Vec<(String, i64)> = Vec::new();
    for row in trend_rows {
        let raw: String = row.get("created_at");
        let parsed = NaiveDateTime::parse_from_str(raw.trim_end_matches('Z'), "%Y-%m-%dT%H:%M:%S");
        let Ok(ndt) = parsed else { continue };
        let utc_dt = DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc);
        let day = utc_dt.with_timezone(tz).format("%Y-%m-%d").to_string();
        match by_day.last_mut() {
            Some((d, c)) if *d == day => *c += 1,
            _ => by_day.push((day, 1)),
        }
    }
    let trend: Vec<DailyCount> = by_day
        .into_iter()
        .map(|(date, count)| DailyCount { date, count })
        .collect();
    Ok(StatsSummary {
        total_views,
        total_posts,
        total_moments,
        total_attachments,
        trend,
    })
}

/// 阅读量 Top 文章（按浏览量倒序），返回 (Post, 期间浏览量)。
pub async fn top_posts(
    db: &Db,
    from: Option<&str>,
    to: Option<&str>,
    limit: i64,
    tz: &Tz,
) -> AppResult<Vec<(Post, i64)>> {
    let (where_sql, binds) = range_filter("pv.created_at", from, to, tz);
    let sql = format!(
        "SELECT {POST_COLUMNS}, COUNT(pv.id) AS view_count \
         FROM posts p JOIN page_views pv ON pv.post_id = p.id \
         {where_sql} \
         GROUP BY p.id ORDER BY view_count DESC, p.id LIMIT ?"
    );
    let mut q = sqlx::query(&sql);
    for b in &binds {
        q = q.bind(b);
    }
    q = q.bind(limit);
    let rows = q.fetch_all(db).await?;
    Ok(rows
        .iter()
        .map(|r| (PostStatRow::from_row(r).expect("行结构匹配").into(), r.get("view_count")))
        .collect())
}

/// 地区阅读量分组（按 country/province/city 汇总，浏览量倒序）。
pub async fn by_region(
    db: &Db,
    from: Option<&str>,
    to: Option<&str>,
    tz: &Tz,
) -> AppResult<Vec<RegionStat>> {
    let (where_sql, binds) = range_filter("created_at", from, to, tz);
    let sql = format!(
        "SELECT country, province, city, COUNT(*) AS count \
         FROM page_views {where_sql} \
         GROUP BY country, province, city ORDER BY count DESC, country, province, city"
    );
    let mut q = sqlx::query(&sql);
    for b in &binds {
        q = q.bind(b);
    }
    let rows = q.fetch_all(db).await?;
    Ok(rows
        .iter()
        .map(|r| RegionStat {
            country: r.get("country"),
            province: r.get("province"),
            city: r.get("city"),
            count: r.get("count"),
        })
        .collect())
}

/// 跳转来源分布：按 source 分类分组，阅读降序（direct 无 referer 通常最多）。
pub async fn by_source(
    db: &Db,
    from: Option<&str>,
    to: Option<&str>,
    tz: &Tz,
) -> AppResult<Vec<SourceStat>> {
    let (where_sql, binds) = range_filter("created_at", from, to, tz);
    let sql = format!(
        "SELECT source, COUNT(*) AS count \
         FROM page_views {where_sql} \
         GROUP BY source ORDER BY count DESC, source"
    );
    let mut q = sqlx::query(&sql);
    for b in &binds {
        q = q.bind(b);
    }
    let rows = q.fetch_all(db).await?;
    Ok(rows
        .iter()
        .map(|r| SourceStat {
            source: r.get("source"),
            count: r.get("count"),
        })
        .collect())
}

/// 清空阅读明细日志（page_views 表；posts.views 累计计数保留）。
pub async fn clear_logs(db: &Db) -> AppResult<()> {
    sqlx::query("DELETE FROM page_views").execute(db).await?;
    Ok(())
}

/// 把可选的 `YYYY-MM-DD` 范围转成 SQL WHERE 片段与绑定值。
/// `from`/`to` 为站点时区下的自然日：下界 = 该日 00:00（本地）对应的 UTC 时刻，
/// 上界 = 次日 00:00（本地）对应的 UTC 时刻（开区间）。created_at 存 UTC ISO
/// 字符串（`YYYY-MM-DDTHH:MM:SSZ`），字典序即时间序，直接按转换后的 UTC 串比较。
fn range_filter(
    col: &str,
    from: Option<&str>,
    to: Option<&str>,
    tz: &Tz,
) -> (String, Vec<String>) {
    let (lower, upper) = crate::services::timezone::local_day_utc_bounds(from, to, tz);
    let mut conds: Vec<String> = Vec::new();
    let mut binds: Vec<String> = Vec::new();
    if let Some(f) = lower {
        conds.push(format!("{col} >= ?"));
        binds.push(f);
    }
    if let Some(t) = upper {
        conds.push(format!("{col} < ?"));
        binds.push(t);
    }
    let where_sql = if conds.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conds.join(" AND "))
    };
    (where_sql, binds)
}

/// posts 表列（带 `p.` 前缀，与 `PostStatRow` 字段顺序一致）。
const POST_COLUMNS: &str = "p.id, p.uuid, p.slug, p.title, p.content_md, p.excerpt, p.status, \
    p.post_type, p.published_at, p.created_at, p.updated_at, p.views, p.like_count, p.category_id";

/// 排行榜行：13 个 post 字段（view_count 不在此结构内，另行 `Row::get` 读取，
/// FromRow 对结果集多余列自动忽略）。
#[derive(FromRow)]
struct PostStatRow {
    id: i64,
    uuid: String,
    slug: String,
    title: String,
    content_md: String,
    excerpt: String,
    status: String,
    post_type: String,
    published_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    views: i64,
    like_count: i64,
    category_id: Option<i64>,
}

impl From<PostStatRow> for Post {
    fn from(r: PostStatRow) -> Self {
        Post {
            id: r.id,
            uuid: r.uuid,
            slug: r.slug,
            title: r.title,
            content_md: r.content_md,
            excerpt: r.excerpt,
            status: PostStatus::from_str(&r.status).unwrap_or_default(),
            post_type: PostType::from_str(&r.post_type).unwrap_or_default(),
            published_at: r.published_at,
            created_at: r.created_at,
            updated_at: r.updated_at,
            views: r.views,
            like_count: r.like_count,
            category_id: r.category_id,
            column_id: None,
            column_sort: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::classify_referer;

    #[test]
    fn classify_wechat() {
        assert_eq!(classify_referer("https://mp.weixin.qq.com/s/abc"), "wechat");
        assert_eq!(classify_referer("https://weixin.qq.com/x"), "wechat");
    }

    #[test]
    fn classify_platforms() {
        assert_eq!(classify_referer("https://www.zhihu.com/question/1"), "zhihu");
        assert_eq!(classify_referer("https://blog.csdn.net/abc/article/1"), "csdn");
        assert_eq!(classify_referer("https://juejin.cn/post/1"), "juejin");
        assert_eq!(classify_referer("https://weibo.com/u/123"), "weibo");
        assert_eq!(classify_referer("https://www.jianshu.com/p/abc"), "jianshu");
        assert_eq!(classify_referer("https://github.com/Angryshark128/hancic-blog"), "github");
    }

    #[test]
    fn classify_search_engines() {
        assert_eq!(classify_referer("https://www.google.com/search?q=x"), "google");
        assert_eq!(classify_referer("https://cn.bing.com/search?q=x"), "bing");
        assert_eq!(classify_referer("https://www.baidu.com/s?wd=x"), "baidu");
    }

    #[test]
    fn classify_direct_and_other() {
        assert_eq!(classify_referer(""), "direct");
        assert_eq!(classify_referer("https://example.com/page"), "other");
    }
}
