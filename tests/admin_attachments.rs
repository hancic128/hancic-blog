//! T15：后台附件库集成测试。
//!
//! 覆盖：登录 → 上传 2 文件（image + file）→ 列表卡片网格含两者 → kind 筛选
//! （image/file，非法值忽略）→ 删除其一 → 列表少一 + 磁盘文件删除。

mod common;
use common::{PNG_1x1, extract_csrf, login_admin, start_server_with_cfg, test_config};
use hancic::db;

/// 后台附件库完整流程：上传 → 列表/筛选 → 删除（DB 行 + 磁盘文件）。
#[tokio::test]
async fn attachment_library_flow() {
    let cfg = test_config("admin-attachments");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    hancic::auth::set_password(&pool, common::TEST_PASSWORD)
        .await
        .unwrap();
    let (addr, client) = start_server_with_cfg(cfg.clone()).await;
    let base = format!("http://{addr}");
    assert!(login_admin(&client, &addr).await);

    // 上传 2 文件：一张 PNG + 一个 txt（不同 kind，供筛选断言）
    let form = reqwest::multipart::Form::new()
        .part(
            "files",
            reqwest::multipart::Part::bytes(PNG_1x1.to_vec())
                .file_name("a.png")
                .mime_str("image/png")
                .unwrap(),
        )
        .part(
            "files",
            reqwest::multipart::Part::bytes(b"hello hancic".to_vec())
                .file_name("b.txt")
                .mime_str("text/plain")
                .unwrap(),
        );
    let res = client
        .post(format!("{base}/api/uploads"))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "上传应 200");
    let json: serde_json::Value = res.json().await.unwrap();
    let data = json["data"].as_array().expect("应含 data 数组");
    assert_eq!(data.len(), 2, "应成功上传两个文件");
    let atts: Vec<(i64, String)> = data
        .iter()
        .map(|a| {
            (
                a["id"].as_i64().expect("附件应有 id"),
                a["path"].as_str().expect("附件应有 path").to_string(),
            )
        })
        .collect();
    let image_id = atts[0].0;
    let file_id = atts[1].0;
    for (_, path) in &atts {
        assert!(
            cfg.data_dir.join("uploads").join(path).exists(),
            "上传后磁盘应有文件 {path}"
        );
    }

    // 列表：卡片网格含两者 + 图片缩略
    let res = client
        .get(format!("{base}/admin/attachments"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "列表应 200");
    let html = res.text().await.unwrap();
    assert!(html.contains("a.png"), "列表应含 a.png");
    assert!(html.contains("b.txt"), "列表应含 b.txt");
    assert!(html.contains("<img"), "图片卡片应含缩略图");
    assert!(
        html.contains(&format!("/uploads/{}", atts[0].1)),
        "缩略图应指向上传文件"
    );

    // kind 筛选：image 只含图片；file 只含文件
    let html = client
        .get(format!("{base}/admin/attachments?kind=image"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(html.contains("a.png"), "kind=image 应含图片");
    assert!(!html.contains("b.txt"), "kind=image 不应含文件");
    let html = client
        .get(format!("{base}/admin/attachments?kind=file"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(html.contains("b.txt"), "kind=file 应含文件");
    assert!(!html.contains("a.png"), "kind=file 不应含图片");

    // 非法 kind 忽略 → 等同无筛选
    let html = client
        .get(format!("{base}/admin/attachments?kind=bogus"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(html.contains("a.png"), "非法 kind 应忽略并显示全部");
    assert!(html.contains("b.txt"), "非法 kind 应忽略并显示全部");

    // 删除图片 → 302 回列表 + 磁盘文件删除 + 列表少一
    let csrf = extract_csrf(&html);
    let res = client
        .post(format!("{base}/admin/attachments/{image_id}/delete"))
        .form(&[("csrf", csrf.as_str())])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302, "删除应 302 回列表");
    assert!(
        !cfg.data_dir.join("uploads").join(&atts[0].1).exists(),
        "删除后磁盘文件应不存在"
    );
    let html = client
        .get(format!("{base}/admin/attachments"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(!html.contains("a.png"), "删除后列表不应含 a.png");
    assert!(html.contains("b.txt"), "未删除的 b.txt 应保留");

    // 删除文件 → 全空 → 占位文案
    let csrf = extract_csrf(&html);
    let res = client
        .post(format!("{base}/admin/attachments/{file_id}/delete"))
        .form(&[("csrf", csrf.as_str())])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302);
    let html = client
        .get(format!("{base}/admin/attachments"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(!html.contains("b.txt"), "删除后列表不应含 b.txt");
    assert!(html.contains("暂无附件"), "空列表应显示占位文案");
}
