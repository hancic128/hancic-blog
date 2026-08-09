//! T5：登录会话、CSRF 与后台鉴权中间件集成测试。
//!
//! 多步登录/CSRF 流程统一走 common::start_server（真实 TCP + cookie 会话客户端）。

mod common;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{extract_csrf, login_admin, setup_password, start_server, test_app};
use tower::ServiceExt;

#[tokio::test]
async fn setup_then_login_flow() {
    let (app, _pool) = test_app("session").await;
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/login")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    // 未设置密码时登录页提示去 setup
    let body = axum::body::to_bytes(res.into_body(), 64 * 1024)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("/admin/setup"), "登录页应提示去 setup");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn login_logout_setup_full_flow() {
    let (addr, client, _pool) = start_server("flow").await;
    let base = format!("http://{addr}");

    // 未登录访问 /admin → 302 到登录页
    let res = client.get(format!("{base}/admin")).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::FOUND);

    // 首次进入 /admin/login → 200 且提示去 setup
    let html = client
        .get(format!("{base}/admin/login"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(html.contains("/admin/setup"));

    // /admin/setup 设置密码 → 302 且自动登录
    assert!(setup_password(&client, &addr).await);
    let res = client.get(format!("{base}/admin")).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::OK, "setup 后应已自动登录");

    // 登出 → /admin 回到登录页
    let res = client
        .get(format!("{base}/admin/logout"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FOUND);
    let res = client.get(format!("{base}/admin")).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::FOUND, "登出后应不可访问 /admin");

    // 密码登录 → 302 → /admin 可访问
    assert!(login_admin(&client, &addr).await);
    let res = client.get(format!("{base}/admin")).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::OK, "密码登录后应可访问 /admin");

    // 密码已设置后 /admin/setup 不可再访问
    let res = client
        .get(format!("{base}/admin/setup"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FOUND);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn login_rejects_bad_csrf() {
    let (addr, client, _pool) = start_server("csrf").await;
    let base = format!("http://{addr}");
    assert!(setup_password(&client, &addr).await);
    let _ = client
        .get(format!("{base}/admin/logout"))
        .send()
        .await
        .unwrap();

    // 错误 CSRF → 302 回登录页且未登录
    let res = client
        .post(format!("{base}/admin/login"))
        .form(&[("password", common::TEST_PASSWORD), ("csrf", "wrong-token")])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FOUND);
    let res = client.get(format!("{base}/admin")).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::FOUND, "错误 CSRF 不应登录成功");

    // 缺少 CSRF → 同样拒绝
    let res = client
        .post(format!("{base}/admin/login"))
        .form(&[("password", common::TEST_PASSWORD)])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FOUND);
    let res = client.get(format!("{base}/admin")).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::FOUND, "缺少 CSRF 不应登录成功");

    // 正确 CSRF → 登录成功
    assert!(login_admin(&client, &addr).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn login_rate_limit_blocks_after_5_failures() {
    let (addr, client, _pool) = start_server("ratelimit").await;
    let base = format!("http://{addr}");
    assert!(setup_password(&client, &addr).await);
    let _ = client
        .get(format!("{base}/admin/logout"))
        .send()
        .await
        .unwrap();

    let html = client
        .get(format!("{base}/admin/login"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let csrf = extract_csrf(&html);

    // 连续 5 次错误密码
    for _ in 0..5 {
        let res = client
            .post(format!("{base}/admin/login"))
            .form(&[("password", "wrong-password"), ("csrf", csrf.as_str())])
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FOUND);
    }

    // 第 6 次即使密码正确也被限流拒绝，且未登录
    let res = client
        .post(format!("{base}/admin/login"))
        .form(&[("password", common::TEST_PASSWORD), ("csrf", csrf.as_str())])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FOUND);
    let res = client.get(format!("{base}/admin")).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::FOUND, "限流后应拒绝登录");
}
