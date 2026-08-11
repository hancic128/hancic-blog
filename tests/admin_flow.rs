//! T12：后台框架（统一布局 + 仪表盘）集成测试。
//!
//! 覆盖：未登录 302、登录后 200 且含仪表盘内容、后台页禁止缓存（no-store）、
//! 布局导航与静态资源管线（/static 下的 css/js/vendor）。

mod common;
use common::{login_admin, start_server_with_cfg, test_config};
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
    assert!(html.contains("阅读趋势"), "仪表盘应含趋势图区块");
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
