//! 阅读统计服务：page_views 写入（含 ip2region 地区）、汇总、排行与清理。

use crate::db::Db;
use crate::error::AppResult;
use crate::ipregion::{Region, Searcher};
use crate::models::{Post, PostStatus, PostType};
use chrono::{DateTime, Utc};
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
        "INSERT INTO page_views(post_id, ip, ua, referer, country, province, city)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(post_id)
    .bind(ip)
    .bind(ua)
    .bind(referer)
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

/// 阅读汇总：`from`/`to` 为 `YYYY-MM-DD`（UTC，当日边界，上界开区间），`None` 不设限。
pub async fn summary(db: &Db, from: Option<&str>, to: Option<&str>) -> AppResult<StatsSummary> {
    let (where_sql, binds) = range_filter("created_at", from, to);
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
    let trend_sql = format!(
        "SELECT substr(created_at, 1, 10) AS date, COUNT(*) AS count \
         FROM page_views {where_sql} GROUP BY date ORDER BY date"
    );
    let mut trend_q = sqlx::query(&trend_sql);
    for b in &binds {
        trend_q = trend_q.bind(b);
    }
    let trend_rows = trend_q.fetch_all(db).await?;
    let trend = trend_rows
        .iter()
        .map(|r| DailyCount {
            date: r.get("date"),
            count: r.get("count"),
        })
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
) -> AppResult<Vec<(Post, i64)>> {
    let (where_sql, binds) = range_filter("pv.created_at", from, to);
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
) -> AppResult<Vec<RegionStat>> {
    let (where_sql, binds) = range_filter("created_at", from, to);
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

/// 清空阅读明细日志（page_views 表；posts.views 累计计数保留）。
pub async fn clear_logs(db: &Db) -> AppResult<()> {
    sqlx::query("DELETE FROM page_views").execute(db).await?;
    Ok(())
}

/// 把可选的 `YYYY-MM-DD` 范围转成 SQL WHERE 片段与绑定值：
/// 下界取当日 0 点（`col >= ?`，字符串前缀比较），上界为次日 0 点开区间
/// （`col < date(?, '+1 day')`），created_at 存的是 UTC ISO 时间，字典序即时间序。
fn range_filter(col: &str, from: Option<&str>, to: Option<&str>) -> (String, Vec<String>) {
    let mut conds: Vec<String> = Vec::new();
    let mut binds: Vec<String> = Vec::new();
    if let Some(f) = from.filter(|f| !f.is_empty()) {
        conds.push(format!("{col} >= ?"));
        binds.push(f.to_string());
    }
    if let Some(t) = to.filter(|t| !t.is_empty()) {
        conds.push(format!("{col} < date(?, '+1 day')"));
        binds.push(t.to_string());
    }
    let where_sql = if conds.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conds.join(" AND "))
    };
    (where_sql, binds)
}

/// posts 表列（带 `p.` 前缀，与 `PostStatRow` 字段顺序一致）。
const POST_COLUMNS: &str = "p.id, p.slug, p.title, p.content_md, p.excerpt, p.status, \
    p.post_type, p.published_at, p.created_at, p.updated_at, p.views, p.category_id";

/// 排行榜行：12 个 post 字段（view_count 不在此结构内，另行 `Row::get` 读取，
/// FromRow 对结果集多余列自动忽略）。
#[derive(FromRow)]
struct PostStatRow {
    id: i64,
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
    category_id: Option<i64>,
}

impl From<PostStatRow> for Post {
    fn from(r: PostStatRow) -> Self {
        Post {
            id: r.id,
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
            category_id: r.category_id,
        }
    }
}
