mod common;
use common::test_app;
use hancic::ipregion::Searcher;
use hancic::models::{PostStatus, PostType};
use hancic::services::{posts, stats};
use chrono_tz::Tz;
use std::net::IpAddr;

/// 私有/保留地址一律归为「本地」。
#[tokio::test]
async fn private_ip_is_local() {
    let searcher = Searcher::new(&common::xdb_path()).unwrap();
    for ip in ["127.0.0.1", "10.1.2.3", "192.168.1.1"] {
        let r = searcher.lookup(&ip.parse::<IpAddr>().unwrap());
        assert_eq!(r.country, "本地", "{ip} 应解析为本地");
    }
}

/// 公网 IP 走 xdb：114.114.114.114（南京电信）国家为中国。
#[tokio::test]
async fn public_ip_resolves_region() {
    let searcher = Searcher::new(&common::xdb_path()).unwrap();
    let r = searcher.lookup(&"114.114.114.114".parse::<IpAddr>().unwrap());
    assert_eq!(r.country, "中国");
}

/// xdb 仅收录 IPv4：IPv6 解析失败同样归「本地」。
#[tokio::test]
async fn ipv6_unknown_is_local() {
    let searcher = Searcher::new(&common::xdb_path()).unwrap();
    let r = searcher.lookup(&"2400:da00::6666".parse::<IpAddr>().unwrap());
    assert_eq!(r.country, "本地");
}

/// record_view 写入（含地区）→ summary/trend/top_posts/by_region 聚合 → clear_logs 清空。
#[tokio::test]
async fn record_view_and_query_summary() {
    let (_app, pool) = test_app("stats").await;
    let searcher = Searcher::new(&common::xdb_path()).unwrap();
    let post = posts::create_post(&pool, posts::NewPost {
        title: "统计文".into(),
        content_md: "# 标题\n正文".into(),
        excerpt: None,
        slug: Some("stats-post".into()),
        status: PostStatus::Published,
        post_type: PostType::Post,
        category_id: None,
        column_id: None,
        tags: vec![],
    })
    .await
    .unwrap();

    // 两次公网（南京电信，同一地区分组）+ 一次私有 IP
    for ip in ["114.114.114.114", "114.114.114.114", "192.168.1.10"] {
        stats::record_view(&pool, post.id, ip, "test-ua", "https://example.com/", &searcher)
            .await
            .unwrap();
    }

    let tz = Tz::Asia__Shanghai;
    let s = stats::summary(&pool, None, None, &tz).await.unwrap();
    assert_eq!(s.total_views, 3);
    assert_eq!(s.total_posts, 1);
    let today = chrono::Utc::now().with_timezone(&tz).format("%Y-%m-%d").to_string();
    assert_eq!(s.trend.iter().map(|d| d.count).sum::<i64>(), 3);
    assert!(
        s.trend.iter().any(|d| d.date == today && d.count == 3),
        "trend 应含当日 3 次阅读，实际 {s:?}"
    );

    let top = stats::top_posts(&pool, None, None, 5, &tz).await.unwrap();
    assert_eq!(top.len(), 1);
    assert_eq!(top[0].0.id, post.id);
    assert_eq!(top[0].1, 3);

    let fresh = posts::get_post(&pool, post.id).await.unwrap().unwrap();
    assert_eq!(fresh.views, 3);

    let regions = stats::by_region(&pool, None, None, &tz).await.unwrap();
    let local = regions.iter().find(|r| r.country == "本地").expect("应有本地分组");
    assert_eq!(local.count, 1);
    let cn = regions.iter().find(|r| r.country == "中国").expect("应有中国分组");
    assert_eq!(cn.count, 2);

    // 时间范围过滤：昨天（本地）无记录
    let yesterday = chrono::Utc::now()
        .with_timezone(&tz)
        .checked_sub_days(chrono::Days::new(1))
        .unwrap()
        .format("%Y-%m-%d")
        .to_string();
    let s2 = stats::summary(&pool, Some(&yesterday), Some(&yesterday), &tz).await.unwrap();
    assert_eq!(s2.total_views, 0);

    // 清空日志后 total_views 归零
    stats::clear_logs(&pool).await.unwrap();
    let s3 = stats::summary(&pool, None, None, &tz).await.unwrap();
    assert_eq!(s3.total_views, 0);
}

/// HTTP 访问文章页：x-real-ip 透传、searcher 解析地区并写入 page_views。
#[tokio::test]
async fn post_page_http_records_view() {
    let (addr, client, pool) = common::start_server("stats-http").await;
    let p = posts::create_post(&pool, posts::NewPost {
        title: "HTTP 统计".into(),
        content_md: "# 标题\n正文".into(),
        excerpt: None,
        slug: Some("http-stats".into()),
        status: PostStatus::Published,
        post_type: PostType::Post,
        category_id: None,
        column_id: None,
        tags: vec![],
    })
    .await
    .unwrap();

    let res = client
        .get(format!("http://{addr}/post/{}", p.uuid))
        .header("x-real-ip", "114.114.114.114")
        .header("user-agent", "test-http-ua")
        .header("referer", "https://example.com/ref")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);

    let count: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM page_views WHERE post_id = (SELECT id FROM posts WHERE slug = 'http-stats')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);

    let row: (String, String, String, String) = sqlx::query_as::<_, (String, String, String, String)>(
        "SELECT country, ip, ua, referer FROM page_views",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.0, "中国");
    assert_eq!(row.1, "114.114.114.114");
    assert_eq!(row.2, "test-http-ua");
    assert_eq!(row.3, "https://example.com/ref");
}
