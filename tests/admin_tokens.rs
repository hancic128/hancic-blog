//! T20：后台 API Token 管理集成测试。
//!
//! 覆盖：生成 Token → 302 到 created 页且明文以 `hc_` 开头、库中仅存 64 位
//! hex 哈希（非明文）；verify(明文)==true；created 页明文仅显示一次（刷新
//! 后不再可得）；吊销后 verify==false；GET /admin/tokens 列表含名称。

mod common;
use common::{extract_csrf, login_admin, start_server_with_cfg, test_config};
use hancic::db;
use hancic::services::tokens;

/// 库中应存 64 位 sha256 hex。
fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

#[tokio::test]
async fn token_generate_display_revoke_flow() {
    let cfg = test_config("admin-tokens");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    hancic::auth::set_password(&pool, common::TEST_PASSWORD)
        .await
        .unwrap();
    let (addr, client) = start_server_with_cfg(cfg).await;
    let base = format!("http://{addr}");
    assert!(login_admin(&client, &addr).await);

    // 列表页可访问，空列表有占位（顺带拿 CSRF）
    let res = client
        .get(format!("{base}/admin/tokens"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "Token 列表页应可访问");
    let html = res.text().await.unwrap();
    let csrf = extract_csrf(&html);
    assert!(html.contains("暂无 Token"), "空列表应有占位");

    // 生成 Token → 302 到 /admin/tokens/{id}/created
    let res = client
        .post(format!("{base}/admin/tokens"))
        .form(&[("name", "CI 部署"), ("csrf", csrf.as_str())])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302, "生成 Token 应 302");
    let location = res
        .headers()
        .get(reqwest::header::LOCATION)
        .expect("应带回跳地址")
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        location.starts_with("/admin/tokens/") && location.ends_with("/created"),
        "应跳到 created 页: {location}"
    );

    // created 页展示明文（仅此一次）
    let res = client
        .get(format!("{base}{location}"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let html = res.text().await.unwrap();
    assert!(html.contains("仅显示一次"), "应提示明文仅显示一次");
    let marker = r#"id="token-plain">"#;
    let start = html.find(marker).expect("created 页应含明文") + marker.len();
    let end = html[start..].find("</code>").expect("明文 code 应闭合") + start;
    let plain = &html[start..end];
    assert!(plain.starts_with("hc_"), "明文应以 hc_ 开头: {plain}");

    // 库中仅存 64 位 hex 哈希（非明文）
    let list = tokens::list(&pool).await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "CI 部署");
    assert_ne!(list[0].token_hash, plain, "库存不应是明文");
    assert!(is_hex64(&list[0].token_hash), "库存应为 64 位 hex");

    // verify(明文) == true；错误明文 == false
    assert!(tokens::verify(&pool, plain).await, "明文应通过校验");
    assert!(!tokens::verify(&pool, "hc_wrong-token").await, "错误明文应校验失败");

    // created 页二次访问不再展示明文（仅显示一次）
    let res = client
        .get(format!("{base}{location}"))
        .send()
        .await
        .unwrap();
    let html = res.text().await.unwrap();
    assert!(!html.contains(plain), "明文不应二次展示");

    // 列表页含名称与有效状态
    let res = client
        .get(format!("{base}/admin/tokens"))
        .send()
        .await
        .unwrap();
    let html = res.text().await.unwrap();
    assert!(html.contains("CI 部署"), "列表应含 Token 名称");
    assert!(html.contains("有效"), "列表应显示有效状态");
    let csrf = extract_csrf(&html);

    // 吊销 → 302 回列表，verify 变 false
    let id = list[0].id;
    let res = client
        .post(format!("{base}/admin/tokens/{id}/revoke"))
        .form(&[("csrf", csrf.as_str())])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302, "吊销应 302 回列表");
    assert!(!tokens::verify(&pool, plain).await, "吊销后 verify 应 false");

    // 列表页显示已吊销
    let res = client
        .get(format!("{base}/admin/tokens"))
        .send()
        .await
        .unwrap();
    let html = res.text().await.unwrap();
    assert!(html.contains("已吊销"), "列表应显示已吊销状态");
}
