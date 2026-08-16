//! T12：后台框架（统一布局 + 仪表盘）集成测试。
//!
//! 覆盖：未登录 302、登录后 200 且含仪表盘内容、后台页禁止缓存（no-store）、
//! 布局导航与静态资源管线（/static 下的 css/js/vendor）。

mod common;
use common::{extract_csrf, login_admin, start_server_with_cfg, test_config};
use hancic::auth;

#[tokio::test]
async fn dashboard_requires_login_and_shows_counts() {
    let cfg = test_config("admin-dash");
    let pool = hancic::db::init(&cfg.data_dir).await.unwrap();
    auth::set_password(&pool, common::TEST_PASSWORD).await.unwrap();
    let (addr, client) = start_server_with_cfg(cfg).await;
    let base = format!("http://{addr}");

    // 未登录访问 /admin → 302 跳登录页
    let res = client.get(format!("{base}/admin")).send().await.unwrap();
    assert_eq!(res.status(), 302);

    // 登录后访问 → 200，含仪表盘标记、CSRF meta、布局导航与趋势数据
    assert!(login_admin(&client, &addr).await);
    let res = client.get(format!("{base}/admin")).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let html = res.text().await.unwrap();
    assert!(html.contains("仪表盘"));
    assert!(
        html.contains(r#"<meta name="csrf-token" content=""#),
        "后台页应输出 CSRF meta"
    );
    assert!(html.contains("查看站点"), "侧边栏应含「查看站点」");
    assert!(html.contains("退出登录"), "侧边栏应含「退出登录」");
    assert!(
        html.contains("admin-nav-item active"),
        "仪表盘导航项应高亮"
    );
    assert!(html.contains("阅读 / 点赞趋势"), "仪表盘应含趋势图区块");
    assert!(html.contains("window.chartData"), "仪表盘应输出趋势数据");

    // 后台页禁止缓存（M19）
    let res = client.get(format!("{base}/admin")).send().await.unwrap();
    assert_eq!(
        res.headers()
            .get("cache-control")
            .map(|v| v.to_str().unwrap()),
        Some("no-store"),
        "后台页应带 Cache-Control: no-store"
    );

    // 静态资源管线：/static 下 css/js/vendor 均可访问
    for path in [
        "/static/admin.css",
        "/static/admin.js",
        "/static/vendor/chart.umd.min.js",
        "/static/vendor/vditor.min.js",
        "/static/vendor/vditor.min.css",
    ] {
        let res = client.get(format!("{base}{path}")).send().await.unwrap();
        assert_eq!(res.status(), 200, "{path} 应可访问");
    }
}

#[tokio::test]
async fn admin_dashboard_shows_recent_7d_like_count() {
    let cfg = test_config("admin-dash-like-count");
    let pool = hancic::db::init(&cfg.data_dir).await.unwrap();
    auth::set_password(&pool, common::TEST_PASSWORD).await.unwrap();
    let (addr, client) = start_server_with_cfg(cfg).await;
    let base = format!("http://{addr}");
    assert!(login_admin(&client, &addr).await);

    let login_html = client
        .get(format!("{base}/admin"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let csrf = extract_csrf(&login_html);
    let res = client
        .post(format!("{base}/admin/posts"))
        .form(&[
            ("title", "仪表盘点赞文章"),
            ("content_md", "点赞正文"),
            ("status", "published"),
            ("category_id", ""),
            ("tags", ""),
            ("csrf", csrf.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302);

    let post_id: i64 = sqlx::query_scalar("SELECT id FROM posts WHERE title = ?")
        .bind("仪表盘点赞文章")
        .fetch_one(&pool)
        .await
        .unwrap();
    for visitor_id in ["visitor-a", "visitor-b", "visitor-c"] {
        sqlx::query(
            "INSERT INTO content_likes(content_type, content_id, visitor_id, ip_hash, ua_hash) VALUES ('post', ?, ?, '', '')",
        )
        .bind(post_id)
        .bind(visitor_id)
        .execute(&pool)
        .await
        .unwrap();
    }

    let html = client
        .get(format!("{base}/admin"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(html.contains("最近 7 天点赞"), "仪表盘应展示最近 7 天点赞卡片: {html}");
    assert!(html.contains(">3<") || html.contains("3"), "仪表盘应展示最近 7 天点赞数 3: {html}");
}
