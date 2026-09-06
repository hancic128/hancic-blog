//! T17：后台站点设置与修改密码集成测试。
//!
//! 覆盖：改 site_name → settings 表更新 + 前台首页 header 显示新名；
//! 非法 nav JSON / 非法时区 / 非法主题模式 → 错误 200 回显且不落库；
//! 修改密码：旧密码错误拒绝、正确后新密码可登录（旧密码失效）。

mod common;
use common::{extract_csrf, login_admin, start_server_with_cfg, test_config};
use hancic::db;
use hancic::services::settings;

/// 设置页表单完整字段（与页面一致），`save` 时拼上 csrf 再提交。
/// 主题模式/时区已移至系统设置页，此处不含。
fn save_fields<'a>(site_name: &'a str, site_nav: &'a str, site_social: &'a str) -> Vec<(&'a str, &'a str)> {
    vec![
        ("site_name", site_name),
        ("site_desc", "测试描述"),
        ("site_nav", site_nav),
        ("site_social", site_social),
    ]
}

/// `save` 提交字段：完整字段 + csrf（reqwest `.form()` 二次调用会覆盖，需一次拼齐）。
fn save_form<'a>(site_name: &'a str, site_nav: &'a str, site_social: &'a str, csrf: &'a str) -> Vec<(&'a str, &'a str)> {
    let mut fields = save_fields(site_name, site_nav, site_social);
    fields.push(("csrf", csrf));
    fields
}

/// 系统设置页表单（主题模式 + 时区）+ csrf。
fn system_form<'a>(theme_mode: &'a str, timezone: &'a str, csrf: &'a str) -> Vec<(&'a str, &'a str)> {
    vec![
        ("theme_mode", theme_mode),
        ("timezone", timezone),
        ("csrf", csrf),
    ]
}

#[tokio::test]
async fn settings_save_updates_db_and_front_header() {
    let cfg = test_config("admin-settings");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    hancic::auth::set_password(&pool, common::TEST_PASSWORD)
        .await
        .unwrap();
    let (addr, client) = start_server_with_cfg(cfg).await;
    let base = format!("http://{addr}");
    assert!(login_admin(&client, &addr).await);

    // 设置页可访问：显示当前站点名与各输入框（顺带拿 CSRF）
    let res = client.get(format!("{base}/admin/settings")).send().await.unwrap();
    assert_eq!(res.status(), 200, "设置页应可访问");
    let html = res.text().await.unwrap();
    let csrf = extract_csrf(&html);
    assert!(html.contains("我的博客"), "设置页应显示当前站点名");
    assert!(html.contains("name=\"site_name\""), "设置页应有站点名输入框");
    assert!(html.contains("name=\"site_nav\""), "设置页应有导航输入框");
    // 修改密码已拆为独立页（左侧菜单），设置页不含密码表单
    assert!(!html.contains("name=\"old_password\""), "设置页不应含修改密码表单");

    // 保存新配置（含导航/社交 JSON、深色模式、东京时区）
    let res = client
        .post(format!("{base}/admin/settings/save"))
        .form(&save_form(
            "寒蝉测试站",
            r#"[{"label":"首页","url":"/"},{"label":"关于","url":"/about"}]"#,
            r#"{"github":"https://github.com/hancic"}"#,
            csrf.as_str(),
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302, "保存成功应 302 回设置页");

    // settings 表逐项更新
    assert_eq!(
        settings::get(&pool, "site_name").await.unwrap().as_deref(),
        Some("寒蝉测试站")
    );
    assert_eq!(
        settings::get(&pool, "site_desc").await.unwrap().as_deref(),
        Some("测试描述")
    );
    assert_eq!(
        settings::get(&pool, "site_nav").await.unwrap().as_deref(),
        Some(r#"[{"label":"首页","url":"/"},{"label":"关于","url":"/about"}]"#)
    );
    assert_eq!(
        settings::get(&pool, "site_social").await.unwrap().as_deref(),
        Some(r#"{"github":"https://github.com/hancic"}"#)
    );
    // 主题模式/时区走系统设置页保存
    let html_sys = client
        .get(format!("{base}/admin/system"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let csrf_sys = extract_csrf(&html_sys);
    let res = client
        .post(format!("{base}/admin/system/save"))
        .form(&system_form("dark", "Asia/Tokyo", csrf_sys.as_str()))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302, "系统设置保存成功应 302 回系统设置页");
    assert_eq!(
        settings::get(&pool, "theme_mode").await.unwrap().as_deref(),
        Some("dark")
    );
    assert_eq!(
        settings::get(&pool, "timezone").await.unwrap().as_deref(),
        Some("Asia/Tokyo")
    );

    // 前台首页 header 显示新站点名 + 新主题模式
    let res = client.get(format!("{base}/")).send().await.unwrap();
    assert_eq!(res.status(), 200, "前台首页应可访问");
    let html = res.text().await.unwrap();
    assert!(html.contains("寒蝉测试站"), "前台 header 应显示新站点名: {html}");
    assert!(html.contains(r#"data-mode="dark""#), "前台应应用深色模式: {html}");
}

#[tokio::test]
async fn invalid_inputs_error_render_and_keep_db() {
    let cfg = test_config("admin-settings-bad");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    hancic::auth::set_password(&pool, common::TEST_PASSWORD)
        .await
        .unwrap();
    let (addr, client) = start_server_with_cfg(cfg).await;
    let base = format!("http://{addr}");
    assert!(login_admin(&client, &addr).await);

    let html = client
        .get(format!("{base}/admin/settings"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let csrf = extract_csrf(&html);

    // 1. 非法 nav JSON（对象而非数组）→ 200 错误回显，保留已填值，不落库
    let res = client
        .post(format!("{base}/admin/settings/save"))
        .form(&save_form("应回显站名", r#"{"label":"首页"}"#, r#"{}"#, csrf.as_str()))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "校验失败应重渲染表单而非跳转");
    let html = res.text().await.unwrap();
    assert!(html.contains("JSON 数组"), "应提示导航格式错误: {html}");
    assert!(html.contains("应回显站名"), "应保留用户已填的站点名: {html}");
    assert_eq!(
        settings::get(&pool, "site_name").await.unwrap().as_deref(),
        Some("我的博客"),
        "非法提交不应落库"
    );

    // 2. 导航数组但某项缺 url → 200 错误回显
    let res = client
        .post(format!("{base}/admin/settings/save"))
        .form(&save_form("站名", r#"[{"label":"首页"}]"#, r#"{}"#, csrf.as_str()))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "缺 url 的导航项应校验失败");
    let html = res.text().await.unwrap();
    assert!(html.contains("缺少链接"), "应提示缺字段: {html}");

    // 3/4. 非法时区 / 非法主题模式 → 200 错误回显（系统设置页）
    let html_sys = client
        .get(format!("{base}/admin/system"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let csrf_sys = extract_csrf(&html_sys);
    let res = client
        .post(format!("{base}/admin/system/save"))
        .form(&system_form("auto", "Not/AZone", csrf_sys.as_str()))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "非法时区应校验失败");
    let html = res.text().await.unwrap();
    assert!(html.contains("时区"), "应提示时区不合法: {html}");

    let res = client
        .post(format!("{base}/admin/system/save"))
        .form(&system_form("neon", "Asia/Shanghai", csrf_sys.as_str()))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "非法主题模式应校验失败");
    let html = res.text().await.unwrap();
    assert!(html.contains("主题模式"), "应提示主题模式不合法: {html}");

    // 5. 空站点名 → 200 错误回显
    let res = client
        .post(format!("{base}/admin/settings/save"))
        .form(&save_form("   ", r#"[]"#, r#"{}"#, csrf.as_str()))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "空站点名应校验失败");
    let html = res.text().await.unwrap();
    assert!(html.contains("站点名称不能为空"), "应提示站点名不能为空: {html}");

    // 全程无任何落库
    assert_eq!(
        settings::get(&pool, "site_name").await.unwrap().as_deref(),
        Some("我的博客"),
        "全部非法提交均不应修改站点名"
    );
}

#[tokio::test]
async fn change_password_flow() {
    let cfg = test_config("admin-settings-pw");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    hancic::auth::set_password(&pool, common::TEST_PASSWORD)
        .await
        .unwrap();
    let (addr, client) = start_server_with_cfg(cfg).await;
    let base = format!("http://{addr}");
    assert!(login_admin(&client, &addr).await);

    // 旧密码错误 → 200 错误回显，哈希未变
    let html = client
        .get(format!("{base}/admin/settings"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let csrf = extract_csrf(&html);
    let old_hash = hancic::auth::get_password_hash(&pool).await.unwrap().unwrap();
    let res = client
        .post(format!("{base}/admin/system/password"))
        .form(&[
            ("old_password", "wrong-old-password"),
            ("new_password", "new-password-456"),
            ("confirm", "new-password-456"),
            ("csrf", csrf.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "旧密码错误应重渲染表单");
    let html = res.text().await.unwrap();
    assert!(html.contains("旧密码不正确"), "应提示旧密码错误: {html}");
    assert_eq!(
        hancic::auth::get_password_hash(&pool).await.unwrap().unwrap(),
        old_hash,
        "旧密码错误时密码哈希不应改变"
    );

    // 新密码过短 → 200 错误回显
    let html = client
        .get(format!("{base}/admin/settings"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let csrf = extract_csrf(&html);
    let res = client
        .post(format!("{base}/admin/system/password"))
        .form(&[
            ("old_password", common::TEST_PASSWORD),
            ("new_password", "short"),
            ("confirm", "short"),
            ("csrf", csrf.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "新密码过短应校验失败");
    let html = res.text().await.unwrap();
    assert!(html.contains("至少"), "应提示新密码长度要求: {html}");

    // 两次输入不一致 → 200 错误回显
    let html = client
        .get(format!("{base}/admin/settings"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let csrf = extract_csrf(&html);
    let res = client
        .post(format!("{base}/admin/system/password"))
        .form(&[
            ("old_password", common::TEST_PASSWORD),
            ("new_password", "new-password-456"),
            ("confirm", "new-password-789"),
            ("csrf", csrf.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "两次输入不一致应校验失败");
    let html = res.text().await.unwrap();
    assert!(html.contains("不一致"), "应提示两次输入不一致: {html}");

    // 正确修改 → 302 回设置页
    let html = client
        .get(format!("{base}/admin/settings"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let csrf = extract_csrf(&html);
    let res = client
        .post(format!("{base}/admin/system/password"))
        .form(&[
            ("old_password", common::TEST_PASSWORD),
            ("new_password", "new-password-456"),
            ("confirm", "new-password-456"),
            ("csrf", csrf.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302, "修改成功应 302 跳登录页（会话已失效）");

    // 登出后用新密码登录
    let res = client.get(format!("{base}/admin/logout")).send().await.unwrap();
    assert_eq!(res.status(), 302, "登出应 302");
    let html = client
        .get(format!("{base}/admin/login"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let csrf = extract_csrf(&html);
    let res = client
        .post(format!("{base}/admin/login"))
        .form(&[("password", "new-password-456"), ("csrf", csrf.as_str())])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302, "新密码应可登录");
    let location = res
        .headers()
        .get(reqwest::header::LOCATION)
        .expect("登录成功应带跳转地址")
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        location.contains("/admin"),
        "新密码登录成功应进入后台: {location}"
    );

    // 登出后旧密码不可登录
    let res = client.get(format!("{base}/admin/logout")).send().await.unwrap();
    assert_eq!(res.status(), 302);
    let html = client
        .get(format!("{base}/admin/login"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let csrf = extract_csrf(&html);
    let res = client
        .post(format!("{base}/admin/login"))
        .form(&[("password", common::TEST_PASSWORD), ("csrf", csrf.as_str())])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302, "旧密码登录应被拒绝");
    let location = res
        .headers()
        .get(reqwest::header::LOCATION)
        .expect("登录失败应带跳转地址")
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        location.contains("error"),
        "旧密码登录失败应跳转错误页: {location}"
    );
}

/// I3：改密成功后既有会话全部失效——旧 cookie 直接访问 /admin 应 302 跳登录，
/// 且改密响应本身跳转登录页（不再回设置页）。
#[tokio::test]
async fn change_password_invalidates_existing_sessions() {
    let cfg = test_config("admin-settings-sess");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    hancic::auth::set_password(&pool, common::TEST_PASSWORD)
        .await
        .unwrap();
    let (addr, client) = start_server_with_cfg(cfg).await;
    let base = format!("http://{addr}");
    assert!(login_admin(&client, &addr).await);

    // 登录后后台可访问（前置：会话有效）
    let res = client.get(format!("{base}/admin")).send().await.unwrap();
    assert_eq!(res.status(), 200, "登录后后台应可访问");

    // 修改密码成功 → 302 跳登录页
    let html = client
        .get(format!("{base}/admin/settings"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let csrf = extract_csrf(&html);
    let res = client
        .post(format!("{base}/admin/system/password"))
        .form(&[
            ("old_password", common::TEST_PASSWORD),
            ("new_password", "new-password-456"),
            ("confirm", "new-password-456"),
            ("csrf", csrf.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302, "修改成功应 302");
    let location = res
        .headers()
        .get(reqwest::header::LOCATION)
        .expect("应带跳转地址")
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        location.contains("/admin/login"),
        "改密后应跳登录页: {location}"
    );

    // 旧 cookie 访问 /admin：会话已全部失效 → 302 跳登录
    let res = client.get(format!("{base}/admin")).send().await.unwrap();
    assert_eq!(res.status(), 302, "改密后旧会话访问 /admin 应 302");
    let location = res
        .headers()
        .get(reqwest::header::LOCATION)
        .expect("应带跳转地址")
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        location.contains("/admin/login"),
        "应跳登录页: {location}"
    );
}
