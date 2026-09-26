//! T12：后台框架（统一布局 + 仪表盘）集成测试。
//!
//! 覆盖：未登录 302、登录后 200 且含仪表盘内容、后台页禁止缓存（no-store）、
//! 布局导航与静态资源管线（/static 下的 css/js/vendor）。

mod common;
use common::{extract_csrf, login_admin, start_server_with_cfg, test_config};
use hancic::auth;

/// 取侧栏底部「最近部署」时间值（`.admin-deploy-time` 元素文本），
/// 形如 `2026-09-26 20:07:03 部署`。
fn extract_admin_deploy_time(html: &str) -> String {
    let marker = "admin-deploy-time";
    let at = html.find(marker).expect("侧栏底部应含 admin-deploy-time 元素");
    let rest = &html[at + marker.len()..];
    let start = rest.find('>').expect("admin-deploy-time 应有起始标签") + 1;
    let end = rest[start..]
        .find('<')
        .expect("admin-deploy-time 应有结束标签");
    rest[start..start + end].trim().to_string()
}

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
    // 值形如「2026-09-26 20:07:03 部署」——含时分秒（曾只显示到日，被截断）
    let deploy = extract_admin_deploy_time(&html);
    let naive = chrono::NaiveDateTime::parse_from_str(&deploy, "%Y-%m-%d %H:%M:%S 部署")
        .unwrap_or_else(|e| panic!("部署时间应形如 YYYY-MM-DD HH:MM:SS 部署（实际 {deploy:?}）: {e}"));
    let tz = chrono_tz::Asia::Shanghai;
    let now = chrono::Utc::now().with_timezone(&tz).naive_local();
    let drift_mins = (now - naive).num_minutes().abs();
    assert!(drift_mins <= 60 * 24, "部署时间应贴近当前时刻（相差 {drift_mins} 分钟）: {deploy}");
    // 版本徽章：CI 用 APP_VERSION build-arg 注入 release tag（如 v1.1.6），
    // 本地未注入时回退到 CARGO_PKG_VERSION（v0.1.0）—— 都符合「v + 三段数字」格式。
    // 取 admin-version-tag 元素后续内容做宽松校验（无 regex 依赖）
    let ver_marker = "admin-version-tag";
    assert!(html.contains(ver_marker), "侧栏底部应含 admin-version-tag 元素");
    let at = html.find(ver_marker).expect("ver_marker 已在上一步定位");
    let after = &html[at + ver_marker.len()..];
    // 找到版本字符串的起止：'>' 后到 '<' 前
    let start = after.find('>').map(|i| at + ver_marker.len() + i + 1);
    let end_rel = start.and_then(|s| after[s - (at + ver_marker.len())..].find('<'));
    let ver_text: String = match (start, end_rel) {
        (Some(s), Some(e)) => html[s..s + e].to_string(),
        _ => String::new(),
    };
    assert!(
        ver_text.starts_with('v')
            && ver_text.chars().filter(|c| c.is_ascii_digit()).count() >= 3
            && ver_text.matches('.').count() >= 2,
        "版本徽章应为「vX.Y.Z」格式，实际 {ver_text:?}"
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

    // 部署时间已移至侧栏底部（admin-meta），格式「YYYY-MM-DD HH:MM:SS 部署」
    let deploy = extract_admin_deploy_time(&html);
    let naive = chrono::NaiveDateTime::parse_from_str(&deploy, "%Y-%m-%d %H:%M:%S 部署")
        .unwrap_or_else(|e| panic!("部署时间应形如 YYYY-MM-DD HH:MM:SS 部署（实际 {deploy:?}）: {e}"));
    // 时区改 UTC 后必须跟着变：与 UTC 当前时刻相差不超过 1 天（容跨午夜边界）
    let now_utc = chrono::Utc::now().naive_utc();
    let drift_mins = (now_utc - naive).num_minutes().abs();
    assert!(
        drift_mins <= 60 * 24,
        "时区设为 UTC 后部署时间应贴近 UTC 当前时刻（相差 {drift_mins} 分钟）: {deploy}"
    );
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
