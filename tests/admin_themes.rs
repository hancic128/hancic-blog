//! T18：后台主题管理集成测试。
//!
//! 覆盖：主题列表含 default 并标记当前；仓库 `themes/test-theme` 夹具
//! （最小 theme.toml + index.html）经 common 复制后 discover 出现在列表；
//! preview 302 到 `/?theme_preview=`，前台按预览主题渲染（模板与静态资源
//! 路径均切换）且 **不落库**（预览在 activate 之前进行：此时
//! active_theme=default、预览 test-theme，若预览写库断言即失败）；
//! activate 写 settings.active_theme 且列表「当前」标记随之迁移（前台模板
//! 渲染器启动时固定，切换需重启完全生效，故只断言 CSS 路径切换）。

mod common;
use common::{extract_csrf, login_admin, start_server_with_cfg, test_config};
use axum::body::Body;
use axum::http::Request;
use hancic::db;
use hancic::services::settings;
use tower::ServiceExt;

#[tokio::test]
async fn theme_admin_flow() {
    let cfg = test_config("admin-themes");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    hancic::auth::set_password(&pool, common::TEST_PASSWORD)
        .await
        .unwrap();
    // 启动即复制仓库 themes/（default + test-theme 夹具）到数据目录；
    // cfg 保留副本供「模拟重启」步骤再次调用 hancic::app
    let (addr, client) = start_server_with_cfg(cfg.clone()).await;
    let base = format!("http://{addr}");
    assert!(login_admin(&client, &addr).await);

    // 1. 列表：含 default（当前标记）与复制进来的 test-theme
    let res = client.get(format!("{base}/admin/themes")).send().await.unwrap();
    assert_eq!(res.status(), 200, "主题列表页应可访问");
    let html = res.text().await.unwrap();
    let csrf = extract_csrf(&html);
    assert!(html.contains("default"), "列表应含 default 主题: {html}");
    assert!(
        html.contains(r#">default <span class="badge-current">当前</span>"#),
        "default 初始应标记为当前主题: {html}"
    );
    assert!(
        html.contains("test-theme"),
        "复制夹具后 discover 应出现 test-theme: {html}"
    );
    assert!(
        !html.contains(r#">test-theme <span class="badge-current">当前</span>"#),
        "test-theme 初始不应标记为当前主题: {html}"
    );

    // 2. preview（activate 之前）：此时 active_theme=default、预览 test-theme
    let res = client
        .get(format!("{base}/admin/themes/test-theme/preview"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302, "预览应 302 到前台");
    let location = res
        .headers()
        .get(reqwest::header::LOCATION)
        .expect("预览成功应带跳转地址")
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        location.contains("/?theme_preview=test-theme"),
        "预览跳转应带主题预览参数: {location}"
    );

    // 3. 预览渲染：用 test-theme 的 tera 与静态资源路径
    let res = client.get(format!("{base}{location}")).send().await.unwrap();
    assert_eq!(res.status(), 200, "预览首页应可访问");
    let html = res.text().await.unwrap();
    assert!(
        html.contains(r#"data-theme="test-theme""#),
        "应渲染预览主题的模板: {html}"
    );
    assert!(
        html.contains(r#"/theme/test-theme/static/style.css"#),
        "预览时静态资源路径应指向预览主题: {html}"
    );
    // 预览不落库：active_theme 仍为 default（若预览写库，此处即失败）
    assert_eq!(
        settings::get(&pool, "active_theme").await.unwrap().as_deref(),
        Some("default"),
        "预览不应改动 active_theme 设置"
    );

    // 4. 预览对非预览渲染无影响：不带参数的前台首页仍是默认主题
    let res = client.get(format!("{base}/")).send().await.unwrap();
    assert_eq!(res.status(), 200, "前台首页应可访问");
    let html = res.text().await.unwrap();
    assert!(
        html.contains(r#"/theme/default/static/style.css"#),
        "非预览请求应渲染默认主题: {html}"
    );

    // 5. activate：POST 写 settings.active_theme，列表「当前」标记迁移
    let res = client
        .post(format!("{base}/admin/themes/test-theme/activate"))
        .form(&[("csrf", csrf.as_str())])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302, "启用主题应 302 回列表");
    let location = res
        .headers()
        .get(reqwest::header::LOCATION)
        .expect("启用成功应带跳转地址")
        .to_str()
        .unwrap()
        .to_string();
    assert!(location.contains("msg="), "应带结果提示参数: {location}");
    assert_eq!(
        settings::get(&pool, "active_theme").await.unwrap().as_deref(),
        Some("test-theme"),
        "activate 应写 settings.active_theme"
    );
    let html = client
        .get(format!("{base}{location}"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(html.contains("已切换到"), "应回显切换成功提示: {html}");
    assert!(
        html.contains(r#">test-theme <span class="badge-current">当前</span>"#),
        "激活后 test-theme 应标记为当前主题: {html}"
    );

    // 6. 激活后未重启：前台 tera 仍为启动时默认主题模板，但 site.active_theme
    //    已切换（CSS 路径指向新主题；模板/布局需重启才完全生效——提示行为）
    let res = client.get(format!("{base}/")).send().await.unwrap();
    assert_eq!(res.status(), 200, "前台首页应可访问");
    let html = res.text().await.unwrap();
    assert!(
        html.contains(r#"/theme/test-theme/static/style.css"#),
        "激活后前台静态资源路径应指向 test-theme: {html}"
    );

    // 7. 模拟重启（C2 修复）：重新 hancic::app()——启动读 settings.active_theme
    //    覆盖 config 默认值，前台模板渲染器应切换到 test-theme 的 index.html
    let app2 = hancic::app(cfg.clone()).await.unwrap();
    let res = app2
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "重启后前台首页应可访问");
    let bytes = axum::body::to_bytes(res.into_body(), 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    let html = String::from_utf8(bytes).unwrap();
    assert!(
        html.contains("Test Theme"),
        "重启后应渲染 test-theme 的 index.html: {html}"
    );
    assert!(
        html.contains(r#"data-theme="test-theme""#),
        "重启后模板应标记 test-theme: {html}"
    );
}
