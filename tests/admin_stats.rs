//! T19：后台统计模块集成测试（统计已合并进仪表盘 /admin）。
//!
//! 覆盖：seed 文章 + 手工 INSERT page_views（2 条不同国家 + 1 条省市）→
//! 仪表盘卡片正确、趋势含当日、地区明细含中国/美国/江苏/南京；
//! 非法 from/to 回落默认区间（页面仍 200）；
//! POST /admin/stats/clear（带 CSRF）后总阅读归零；历史 /admin/stats 重定向到仪表盘。

mod common;
use common::{extract_csrf, login_admin, start_server};
use chrono_tz::Tz;
use hancic::models::{PostStatus, PostType};
use hancic::services::posts;

/// 统计全链路：仪表盘统计区 → 地区明细 → 清理归零。
#[tokio::test]
async fn stats_overview_region_detail_and_clear() {
    let (addr, client, pool) = start_server("admin-stats").await;
    hancic::auth::set_password(&pool, common::TEST_PASSWORD)
        .await
        .unwrap();
    assert!(login_admin(&client, &addr).await);
    let base = format!("http://{addr}");

    // seed 两篇文章
    let p1 = posts::create_post(
        &pool,
        posts::NewPost {
            title: "统计文章一".into(),
            content_md: "# 标题\n正文".into(),
            excerpt: None,
            slug: Some("stats-a".into()),
            status: PostStatus::Published,
            post_type: PostType::Post,
            category_id: None,
            column_id: None,
            tags: vec![],
        },
    )
    .await
    .unwrap();
    let p2 = posts::create_post(
        &pool,
        posts::NewPost {
            title: "统计文章二".into(),
            content_md: "# 标题\n正文".into(),
            excerpt: None,
            slug: Some("stats-b".into()),
            status: PostStatus::Published,
            post_type: PostType::Post,
            category_id: None,
            column_id: None,
            tags: vec![],
        },
    )
    .await
    .unwrap();

    // 手工 INSERT page_views：2 条不同国家（中国/美国）+ 1 条中国·江苏·南京。
    // created_at 用默认值（UTC now），趋势按 UTC 日期分组 → 归入当日。
    for (post_id, country, province, city) in [
        (p1.id, "中国", "", ""),
        (p2.id, "美国", "", ""),
        (p1.id, "中国", "江苏", "南京"),
    ] {
        sqlx::query(
            "INSERT INTO page_views(post_id, ip, ua, referer, country, province, city)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(post_id)
        .bind("1.2.3.4")
        .bind("test-ua")
        .bind("https://example.com/")
        .bind(country)
        .bind(province)
        .bind(city)
        .execute(&pool)
        .await
        .unwrap();
    }

    // 手工 INSERT 两条点赞记录（当日），供点赞趋势断言
    for content_id in [p1.id, p2.id] {
        sqlx::query(
            "INSERT INTO content_likes(content_type, content_id, visitor_id, ip_hash, ua_hash)
             VALUES ('post', ?, ?, '', '')",
        )
        .bind(content_id)
        .bind(format!("visitor-{content_id}"))
        .execute(&pool)
        .await
        .unwrap();
    }

    // 历史 /admin/stats 直接渲染仪表盘（统计已合并）
    let res = client.get(format!("{base}/admin/stats")).send().await.unwrap();
    assert_eq!(res.status(), 200, "历史统计路由应直接渲染仪表盘");

    // 仪表盘：总阅读卡片 = 3，趋势横轴含当日
    let res = client.get(format!("{base}/admin")).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let html = res.text().await.unwrap();
    assert!(
        html.contains(r#"<div class="stat-num">3</div>"#),
        "总阅读卡片应为 3: {html}"
    );
    let tz = Tz::Asia__Shanghai;
    let today = chrono::Utc::now().with_timezone(&tz).format("%Y-%m-%d").to_string();
    assert!(html.contains(&today), "趋势横轴应含站点时区当日 {today}");
    // 点赞趋势：当日点赞数 = 2（两条 content_likes 记录），chart_data 双数据集
    let chart_start = html.find("window.chartData = ").map(|i| i + "window.chartData = ".len())
        .expect("仪表盘应输出 chartData");
    let chart_json = html[chart_start..].split("</script>").next().unwrap_or("").trim();
    let chart_json = chart_json.strip_suffix(';').unwrap_or(chart_json);
    let chart: serde_json::Value = serde_json::from_str(chart_json)
        .unwrap_or_else(|e| panic!("chartData 应为合法 JSON: {e}"));
    assert!(chart.get("views").is_some(), "趋势数据应含阅读序列");
    let likes = chart.get("likes").and_then(|v| v.as_array()).expect("趋势数据应含点赞序列");
    assert_eq!(likes.last().and_then(|v| v.as_i64()), Some(2),
        "点赞趋势当日应为 2: {likes:?}");
    assert!(html.contains("阅读 / 点赞趋势"), "趋势标题应标注阅读与点赞");

    // 文章排行：两篇文章都在仪表盘排行区
    assert!(html.contains("统计文章一") && html.contains("统计文章二"), "排行应含两篇文章");

    // 地区明细：国家/省份两列（城市并入省份），中国/美国/江苏可见
    assert!(html.contains("中国") && html.contains("美国"), "地区应含中国与美国");
    assert!(html.contains("江苏"), "地区应含江苏（城市并入省份）");
    assert!(html.contains("<th>国家</th>") && html.contains("<th>省份</th>"), "地区应国家/省份两列表头");
    assert!(!html.contains("<th>城市</th>"), "地区不应再有城市列");
    // 地区地图：中国省份以 choropleth 展示（容器 + 注入 JSON）
    assert!(html.contains(r#"id="region-map""#), "有中国省份数据时应渲染地图容器");
    assert!(html.contains("window.regionData = "), "应注入地区地图数据 JSON");
    // 文章排行：固定 Top 10，无分页控件
    assert!(!html.contains("上一页") && !html.contains("下一页"), "Top10 排行不应分页");

    // 「全部」范围（?range=all）：表单留空 + 快捷钮高亮「全部」
    let res = client
        .get(format!("{base}/admin?range=all"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let html = res.text().await.unwrap();
    assert!(
        html.contains(r#"class="quick-range active" href="?range=all""#),
        "全部快捷钮应高亮: {html}"
    );
    assert!(
        html.contains(r#"name="from" class="date-input" value=""#) && html.contains(r#"name="to" class="date-input" value=""#),
        "全部模式下 from/to 输入框应留空"
    );
    assert!(
        html.contains("window.regionData = [") && html.contains("window.chartSources = ["),
        "全部模式仍应注入地区与来源数据"
    );

    // 非法 from/to：回落默认区间，页面仍 200
    let res = client
        .get(format!("{base}/admin?from=2026/01/01"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "非法 from 应回落默认区间而非 400");

    // 清理：POST 带 CSRF → 302 回仪表盘；总阅读归零
    let html = client
        .get(format!("{base}/admin"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let csrf = extract_csrf(&html);
    let res = client
        .post(format!("{base}/admin/stats/clear"))
        .form(&[("csrf", csrf.as_str())])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302, "清理后应 302 回仪表盘");
    let html = client
        .get(format!("{base}/admin"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        html.contains(r#"<div class="stat-num">0</div>"#),
        "清理后总阅读应为 0: {html}"
    );
}
