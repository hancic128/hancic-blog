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
    // 品牌区：点击 logo/站名在新标签页打开博客首页（target=_blank + rel=noopener）
    assert!(
        html.contains(r#"class="admin-brand" href="/" target="_blank" rel="noopener""#),
        "品牌区应为指向博客首页的新标签页链接"
    );
    // 侧栏底部小字：按系统设置/时区（默认 Asia/Shanghai）换算的最近部署时间；
    // 格式「YYYY-MM-DD 部署」（去掉了时分秒以适配侧栏底部窄列）。
    let marker = "最近部署 ";
    let at = html.find(marker).expect("admin-meta 应含「最近部署」小字");
    let raw = &html[at + marker.len()..];
    let raw = raw.chars().take(10).collect::<String>(); // YYYY-MM-DD
    let naive_date = chrono::NaiveDate::parse_from_str(&raw, "%Y-%m-%d")
        .unwrap_or_else(|e| panic!("部署日期应形如 YYYY-MM-DD（实际 {raw:?}）: {e}"));
    let tz = chrono_tz::Asia::Shanghai;
    let today = chrono::Utc::now().with_timezone(&tz).date_naive();
    let drift_days = (today - naive_date).num_days().abs();
    assert!(drift_days <= 1, "部署日期应贴近当天（相差 {drift_days} 天）: {raw}");
    // 版本徽章：编译期固化（CARGO_PKG_VERSION），前缀 v
    let ver_marker = "admin-version-tag";
    assert!(
        html.contains(ver_marker) && html.contains("v0.1.0"),
        "侧栏底部应含版本徽章 v0.1.0"
    );
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
async fn admin_brand_deploy_time_follows_site_timezone() {
    let cfg = test_config("admin-brand-tz");
    let pool = hancic::db::init(&cfg.data_dir).await.unwrap();
    auth::set_password(&pool, common::TEST_PASSWORD).await.unwrap();
    // 系统设置里把时区改成 UTC：品牌区小字必须跟着变（而不是固定 UTC+8）
    sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES ('timezone', 'UTC')")
        .execute(&pool)
        .await
        .unwrap();
    let (addr, client) = start_server_with_cfg(cfg).await;
    let base = format!("http://{addr}");
    assert!(login_admin(&client, &addr).await);
    let html = client
        .get(format!("{base}/admin"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    // 部署时间已移至侧栏底部（admin-meta），格式「YYYY-MM-DD 部署」
    let marker = "最近部署 ";
    let at = html.find(marker).expect("admin-meta 应含「最近部署」小字");
    // 跳过「最近部署 」+ 「YYYY-MM-DD 」+ 「部署」前缀，取剩下的纯日期字符串
    let after = &html[at + marker.len()..];
    let date_str: String = after.chars().take(10).collect();
    let naive = chrono::NaiveDate::parse_from_str(&date_str, "%Y-%m-%d")
        .unwrap_or_else(|e| panic!("部署时间应形如 YYYY-MM-DD（实际 {date_str:?}）: {e}"));
    // 用当天 00:00:00 当作时间锚：允许 ±1 天的漂移（时区跨午夜边界）
    use chrono::TimeZone as _;
    let now_utc = chrono::Utc::now();
    let shown = chrono::Utc.from_utc_datetime(&naive.and_hms_opt(0, 0, 0).unwrap());
    let drift_days = (now_utc.date_naive() - shown.date_naive()).num_days().abs();
    assert!(drift_days <= 1, "时区设为 UTC 后部署日期应贴近 UTC 当天（相差 {drift_days} 天）: {date_str}");
}

#[tokio::test]
async fn admin_dashboard_shows_total_like_count() {
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
    assert!(html.contains("总点赞"), "仪表盘应展示累计点赞卡片: {html}");
    assert!(html.contains(r#"<div class="stat-num">3</div>"#), "仪表盘应展示累计点赞数 3: {html}");
}
