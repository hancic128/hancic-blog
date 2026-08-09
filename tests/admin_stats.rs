//! T19：后台统计模块集成测试。
//!
//! 覆盖：seed 文章 + 手工 INSERT page_views（2 条不同国家 + 1 条省市）→
//! 总览卡片正确、趋势含当日；地区含「中国」且国家→省→市下钻正确；
//! from/to 非法 → 400；POST /admin/stats/clear（带 CSRF）后总览归零。

mod common;
use common::{extract_csrf, login_admin, start_server};
use hancic::models::{PostStatus, PostType};
use hancic::services::posts;

/// 统计全链路：总览 → 排行 → 地区下钻 → 清理归零。
#[tokio::test]
async fn stats_overview_region_drilldown_and_clear() {
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

    // 总览：总阅读卡片 = 3，趋势横轴含当日
    let res = client.get(format!("{base}/admin/stats")).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let html = res.text().await.unwrap();
    assert!(
        html.contains(r#"<div class="stat-num">3</div>"#),
        "总阅读卡片应为 3: {html}"
    );
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    assert!(html.contains(&today), "趋势横轴应含当日 {today}");

    // 排行页 200
    let res = client
        .get(format!("{base}/admin/stats/posts"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);

    // 地区国家层：含中国与美国
    let res = client
        .get(format!("{base}/admin/stats/regions"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let html = res.text().await.unwrap();
    assert!(html.contains("中国") && html.contains("美国"), "国家层应含中国与美国");

    // 下钻国家 → 省份层含江苏
    let res = client
        .get(format!("{base}/admin/stats/regions?country={}", urlencode("中国")))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let html = res.text().await.unwrap();
    assert!(html.contains("江苏"), "省份层应含江苏: {html}");

    // 下钻省份 → 城市层含南京
    let res = client
        .get(format!(
            "{base}/admin/stats/regions?country={}&province={}",
            urlencode("中国"),
            urlencode("江苏")
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let html = res.text().await.unwrap();
    assert!(html.contains("南京"), "城市层应含南京: {html}");

    // from/to 非法 → 400 提示
    let res = client
        .get(format!("{base}/admin/stats?from=2026/01/01"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 400, "非法 from 应 400");

    // 清理：POST 带 CSRF → 302 回统计页；总览归零
    let html = client
        .get(format!("{base}/admin/stats"))
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
    assert_eq!(res.status(), 302, "清理后应 302 回统计页");
    let html = client
        .get(format!("{base}/admin/stats"))
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

/// 极简百分号编码（与仓库内其他测试一致），用于拼接含中文的 query 参数。
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
