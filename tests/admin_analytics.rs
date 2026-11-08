//! 访问分析（/admin/analytics + /api/analytics）集成测试。
//!
//! 覆盖：
//! - UA 解析与设备分类（桌面/移动/平板，爬虫被剔除出「真实阅读」）
//! - 独立访客数与新客/回访划分
//! - 来源域名明细、搜索关键词抽取
//! - 时段热力（7×24 格）
//! - 单篇来源下钻
//! - 后台页渲染（各板块标题 + 注入的图表数据）
//! - REST API 返回同一套口径

mod common;
use chrono::{Duration, SecondsFormat, Utc};
use common::{login_admin, start_server};
use hancic::models::{PostStatus, PostType};
use hancic::services::posts;

/// 桌面 Chrome UA。
const UA_DESKTOP: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
/// iPhone Safari UA。
const UA_MOBILE: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";
/// iPad UA。
const UA_TABLET: &str = "Mozilla/5.0 (iPad; CPU OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";
/// 搜索引擎爬虫 UA。
const UA_BOT: &str = "Mozilla/5.0 (compatible; Googlebot/2.1; +http://www.google.com/bot.html)";

/// 造一篇已发布文章，返回 id。
async fn seed_post(pool: &hancic::db::Db, slug: &str, title: &str) -> i64 {
    posts::create_post(
        pool,
        posts::NewPost {
            title: title.into(),
            content_md: "正文".into(),
            excerpt: None,
            slug: Some(slug.into()),
            status: PostStatus::Published,
            post_type: PostType::Post,
            category_id: None,
            column_id: None,
            tags: vec![],
        },
    )
    .await
    .unwrap()
    .id
}

/// 写入一条访问记录（走真实写入路径，确保分析字段被正确解析落库）。
async fn insert_view(
    pool: &hancic::db::Db,
    post_id: i64,
    ip: &str,
    ua: &str,
    referer: &str,
    created_at: Option<&str>,
) {
    let ua_info = hancic::services::ua::parse(ua);
    let ref_info = hancic::services::referer::parse(referer);
    let visitor_id = hancic::db::visitor_fingerprint(ip, ua);
    sqlx::query(
        "INSERT INTO page_views(post_id, ip, ua, referer, source, country, province, city,
                                device, browser, os, ref_domain, ref_path, search_query, visitor_id, created_at)
         VALUES (?, ?, ?, ?, ?, '', '', '', ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(post_id)
    .bind(ip)
    .bind(ua)
    .bind(referer)
    .bind(hancic::services::stats::classify_referer(referer))
    .bind(ua_info.device_type.as_str())
    .bind(&ua_info.browser)
    .bind(&ua_info.os)
    .bind(&ref_info.domain)
    .bind(&ref_info.path)
    .bind(&ref_info.search_query)
    .bind(&visitor_id)
    .bind(created_at.map(|s| s.to_string()).unwrap_or_else(|| {
        Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
    }))
    .execute(pool)
    .await
    .unwrap();
}

/// 访问分析页各维度：爬虫剔除、设备分类、UV、新老客、来源、关键词、热力。
#[tokio::test]
async fn analytics_page_reports_all_dimensions() {
    let (addr, client, pool) = start_server("admin-analytics").await;
    hancic::auth::set_password(&pool, common::TEST_PASSWORD)
        .await
        .unwrap();
    assert!(login_admin(&client, &addr).await);
    let base = format!("http://{addr}");

    let post = seed_post(&pool, "analytics-post", "分析文章").await;

    // 2 次桌面（不同 IP → 2 个访客）+ 1 次移动 + 1 次平板 + 2 次爬虫
    for (ip, ua) in [
        ("1.1.1.1", UA_DESKTOP),
        ("2.2.2.2", UA_DESKTOP),
        ("3.3.3.3", UA_MOBILE),
        ("4.4.4.4", UA_TABLET),
        ("5.5.5.5", UA_BOT),
        ("6.6.6.6", UA_BOT),
    ] {
        insert_view(&pool, post, ip, ua, "", None).await;
    }
    // 搜索引擎来源（带关键词）+ 普通外链
    insert_view(
        &pool,
        post,
        "7.7.7.7",
        UA_DESKTOP,
        "https://www.google.com/search?q=rust+blog",
        None,
    )
    .await;
    insert_view(
        &pool,
        post,
        "8.8.8.8",
        UA_DESKTOP,
        "https://mp.weixin.qq.com/s/abc",
        None,
    )
    .await;

    let res = client
        .get(format!("{base}/admin/analytics"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "访问分析页应 200");
    let html = res.text().await.unwrap();

    // 各板块标题
    for title in [
        "访问分析",
        "设备构成",
        "浏览器与操作系统",
        "来源明细",
        "搜索关键词",
        "时段热力",
        "单篇来源下钻",
        "实时访问流",
    ] {
        assert!(html.contains(title), "应含板块「{title}」");
    }

    // 真实阅读 = 8 - 2 爬虫 = 6
    assert!(
        html.contains(r#"<div class="stat-num">6</div>"#),
        "真实阅读应剔除爬虫后为 6: {html}"
    );
    // 独立访客与「真实阅读」同口径（不计爬虫）：8 个访客中 2 个是爬虫 → 6
    let visitors_section = html
        .find("独立访客")
        .map(|i| html[i.saturating_sub(300)..i].to_string())
        .unwrap_or_default();
    assert!(
        visitors_section.contains(r#"<div class="stat-num">6</div>"#),
        "独立访客应与真实阅读同口径为 6: {visitors_section}"
    );

    // 设备标签齐全（桌面端/移动端/平板）
    for label in ["桌面端", "移动端", "平板"] {
        assert!(html.contains(label), "设备表应含 {label}: {html}");
    }

    // 来源域名明细应含 google.com 与 mp.weixin.qq.com
    assert!(
        html.contains("www.google.com") && html.contains("mp.weixin.qq.com"),
        "来源域名应含完整域名: {html}"
    );

    // 搜索关键词应被抽取并展示
    assert!(html.contains("rust blog"), "应抽取搜索关键词: {html}");

    // 注入的图表数据应含 heatmap（7×24 = 168 格）
    assert!(
        html.contains("window.chartData = "),
        "应注入图表数据: {html}"
    );
    let chart_start = html
        .find("window.chartData = ")
        .map(|i| i + "window.chartData = ".len())
        .expect("应注入 chartData");
    let chart_json = html[chart_start..]
        .split("</script>")
        .next()
        .unwrap()
        .trim()
        .trim_end_matches(';');
    let chart: serde_json::Value = serde_json::from_str(chart_json).unwrap();
    let heatmap = chart["heatmap"].as_array().expect("应含 heatmap");
    assert_eq!(heatmap.len(), 168, "热力图应为 7×24 = 168 格");
    // 真实阅读序列合计 = 6
    let views_sum: i64 = chart["views"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_i64())
        .sum();
    assert_eq!(views_sum, 6, "趋势阅读合计应剔除爬虫: {chart:?}");
}

/// 新老访客划分：区间前访问过的算回访，区间内首次的算新客。
#[tokio::test]
async fn analytics_splits_new_and_returning_visitors() {
    let (addr, client, pool) = start_server("admin-analytics-uv").await;
    hancic::auth::set_password(&pool, common::TEST_PASSWORD)
        .await
        .unwrap();
    assert!(login_admin(&client, &addr).await);
    let base = format!("http://{addr}");

    let post = seed_post(&pool, "analytics-uv", "访客文章").await;
    let old = (Utc::now() - Duration::days(40)).to_rfc3339_opts(SecondsFormat::Secs, true);
    let recent = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);

    // 老访客：40 天前访问过一次，区间内又来一次
    insert_view(&pool, post, "10.0.0.1", UA_DESKTOP, "", Some(&old)).await;
    insert_view(&pool, post, "10.0.0.1", UA_DESKTOP, "", Some(&recent)).await;
    // 新访客：区间内首次
    insert_view(&pool, post, "10.0.0.2", UA_DESKTOP, "", Some(&recent)).await;

    // 只看最近 30 天
    let html = client
        .get(format!("{base}/admin/analytics?days=30"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    // 区间内 2 个访客：1 个回访 + 1 个新访客
    assert!(
        html.contains(r#"<div class="stat-num">2</div>"#),
        "区间内独立访客应为 2: {html}"
    );
    // 新访客 = 1（10.0.0.2 全局首次落在区间内）
    let new_section = html
        .find("新访客")
        .map(|i| html[i.saturating_sub(300)..i].to_string())
        .unwrap_or_default();
    assert!(
        new_section.contains(r#"<div class="stat-num">1</div>"#),
        "新访客应为 1: {new_section}"
    );
    // 回访访客 = 1（10.0.0.1 全局首次在 40 天前）
    let ret_section = html
        .find("回访访客")
        .map(|i| html[i.saturating_sub(300)..i].to_string())
        .unwrap_or_default();
    assert!(
        ret_section.contains(r#"<div class="stat-num">1</div>"#),
        "回访访客应为 1: {ret_section}"
    );
}

/// 单篇来源下钻：每篇文章各自的来源构成。
#[tokio::test]
async fn analytics_breaks_down_sources_per_post() {
    let (addr, client, pool) = start_server("admin-analytics-drill").await;
    hancic::auth::set_password(&pool, common::TEST_PASSWORD)
        .await
        .unwrap();
    assert!(login_admin(&client, &addr).await);
    let base = format!("http://{addr}");

    let post = seed_post(&pool, "analytics-drill", "来源下钻文章").await;
    insert_view(
        &pool,
        post,
        "20.0.0.1",
        UA_DESKTOP,
        "https://mp.weixin.qq.com/s/x",
        None,
    )
    .await;
    insert_view(
        &pool,
        post,
        "20.0.0.2",
        UA_DESKTOP,
        "https://mp.weixin.qq.com/s/y",
        None,
    )
    .await;
    insert_view(
        &pool,
        post,
        "20.0.0.3",
        UA_DESKTOP,
        "https://www.google.com/search?q=x",
        None,
    )
    .await;

    let html = client
        .get(format!("{base}/admin/analytics"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    // 下钻表应含文章名与来源中文名
    assert!(html.contains("来源下钻文章"), "下钻应含文章标题: {html}");
    assert!(
        html.contains("微信公众号") && html.contains("Google 搜索"),
        "下钻应含来源中文名: {html}"
    );
}

/// 站内互链：站内 Referer 能被识别，且**不会被直接访问挤掉**。
///
/// 回归用例：此前实现是「SQL 先取 Top N、再在应用层按域名过滤」——直接
/// 访问（无 Referer，量最大）会把名额占满，导致站内链接一条都取不到。
/// 现在域名过滤下沉到 SQL，站内链接必须真的出现在结果里。
#[tokio::test]
async fn analytics_detects_internal_links_despite_direct_traffic() {
    let (addr, client, pool) = start_server("admin-analytics-internal").await;
    hancic::auth::set_password(&pool, common::TEST_PASSWORD)
        .await
        .unwrap();
    assert!(login_admin(&client, &addr).await);
    let base = format!("http://{addr}");

    let post = seed_post(&pool, "analytics-internal", "站内互链文章").await;
    // 大量直接访问（量最大，若先取 Top N 会占满名额）
    for i in 0..30 {
        insert_view(&pool, post, &format!("40.0.0.{i}"), UA_DESKTOP, "", None).await;
    }
    // 少量站内 Referer
    for i in 0..3 {
        insert_view(
            &pool,
            post,
            &format!("41.0.0.{i}"),
            UA_DESKTOP,
            "https://example.test/some-page",
            None,
        )
        .await;
    }

    // site_url 为测试配置里的 https://example.test → 站内判定成立
    let res = client
        .get(format!("{base}/admin/analytics"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let html = res.text().await.unwrap();

    assert!(
        html.contains("站内互链"),
        "有站内链接时应显示该板块: {html}"
    );
    assert!(
        html.contains("/some-page"),
        "站内来源路径应出现在互链表: {html}"
    );
    assert!(html.contains("站内互链文章"), "互链表应含目标文章: {html}");
}

/// REST API 返回同一套访问分析口径，且需鉴权。
#[tokio::test]
async fn analytics_api_requires_auth_and_returns_dimensions() {
    let (addr, client, pool) = start_server("admin-analytics-api").await;
    let base = format!("http://{addr}");

    // 未登录 → 401
    let res = client
        .get(format!("{base}/api/analytics"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 401, "未登录应 401");

    hancic::auth::set_password(&pool, common::TEST_PASSWORD)
        .await
        .unwrap();
    assert!(login_admin(&client, &addr).await);

    let post = seed_post(&pool, "analytics-api", "API 文章").await;
    insert_view(
        &pool,
        post,
        "30.0.0.1",
        UA_DESKTOP,
        "https://www.baidu.com/s?wd=%E6%B5%8B%E8%AF%95",
        None,
    )
    .await;
    insert_view(&pool, post, "30.0.0.2", UA_BOT, "", None).await;

    let res = client
        .get(format!("{base}/api/analytics"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let body: serde_json::Value = res.json().await.unwrap();
    let data = &body["data"];
    // 真实阅读 1（爬虫被剔除）
    assert_eq!(
        data["summary"]["total_views"].as_i64(),
        Some(1),
        "API 真实阅读应剔除爬虫: {data}"
    );
    assert_eq!(
        data["summary"]["bot_views"].as_i64(),
        Some(1),
        "API 应单独返回爬虫量: {data}"
    );
    // 关键词解码正确
    let keywords = data["keywords"].as_array().expect("应含关键词数组");
    assert!(
        keywords.iter().any(|k| k["keyword"] == "测试"),
        "关键词应解码为「测试」: {keywords:?}"
    );
    // 来源域名
    let domains = data["referer_domains"]
        .as_array()
        .expect("应含来源域名数组");
    assert!(
        domains.iter().any(|d| d["key"] == "www.baidu.com"),
        "应含来源域名 www.baidu.com: {domains:?}"
    );
    // 热力图 168 格
    assert_eq!(
        data["heatmap"].as_array().map(|a| a.len()),
        Some(168),
        "热力图应为 168 格"
    );
}

/// 未登录访问后台「访问分析」页应跳登录。
#[tokio::test]
async fn analytics_page_redirects_when_not_logged_in() {
    let (addr, client, _pool) = start_server("admin-analytics-auth").await;
    let res = client
        .get(format!("http://{addr}/admin/analytics"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302, "未登录应 302 跳登录");
}
