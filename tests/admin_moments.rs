//! T14：后台说说管理集成测试。
//!
//! 覆盖：上传两图 → 发布说说（content + attachment_ids）→ 后台列表含内容与
//! 缩略图 img → 删除后列表与 DB 均消失。

mod common;
use common::{extract_csrf, login_admin, start_server_with_cfg, test_config, PNG_1x1};
use hancic::db;
use hancic::services::moments;

/// 后台说说完整流程：登录 → 上传 2 图 → POST 发布 → list_moments 断言 →
/// GET /admin/moments 含内容与缩略 img → 删除后消失。
#[tokio::test]
async fn publish_and_delete_flow() {
    let cfg = test_config("admin-moments");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    hancic::auth::set_password(&pool, common::TEST_PASSWORD)
        .await
        .unwrap();
    let (addr, client) = start_server_with_cfg(cfg).await;
    let base = format!("http://{addr}");
    assert!(login_admin(&client, &addr).await);

    // 上传两图（multipart 字段名 files，与 /api/uploads 约定一致）
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
            reqwest::multipart::Part::bytes(PNG_1x1.to_vec())
                .file_name("b.png")
                .mime_str("image/png")
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
    assert_eq!(data.len(), 2, "应成功上传两张图");
    let ids: Vec<i64> = data
        .iter()
        .map(|a| a["id"].as_i64().expect("附件应有 id"))
        .collect();
    let attachment_ids = ids
        .iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(",");

    // 列表页拿 CSRF（顺带验证列表页可访问）
    let res = client.get(format!("{base}/admin/moments")).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let html = res.text().await.unwrap();
    let csrf = extract_csrf(&html);

    // 发布说说
    let content = "后台发布的第一条说说";
    let res = client
        .post(format!("{base}/admin/moments"))
        .form(&[
            ("content", content),
            ("attachment_ids", attachment_ids.as_str()),
            ("csrf", csrf.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302, "发布应 302 回列表");

    // 服务层断言：一条说说 + 两条附件（保持上传顺序）
    let (moments_list, total) = moments::list_moments(&pool, None, false, None, 1, 20).await.unwrap();
    assert_eq!(total, 1);
    let m = &moments_list[0];
    assert_eq!(m.content, content);
    let atts = moments::list_moment_attachments(&pool, m.id).await.unwrap();
    assert_eq!(atts.len(), 2, "说说应关联两张图");
    assert_eq!(atts[0].0.kind, hancic::models::AttachmentKind::Image);
    assert_eq!(atts[1].0.id, ids[1], "附件顺序应按上传顺序");

    // 列表页含内容与缩略图
    let res = client.get(format!("{base}/admin/moments")).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let html = res.text().await.unwrap();
    assert!(html.contains(content), "列表应含说说内容");
    assert!(html.contains("<img"), "列表应含缩略图 img");
    assert!(
        html.contains(&format!("/uploads/{}", atts[0].0.path)),
        "缩略图应指向上传文件"
    );
    assert!(
        html.contains(&format!("/uploads/{}", atts[1].0.path)),
        "两张图的缩略都应出现"
    );

    // 删除 → 列表与 DB 均消失
    let csrf = extract_csrf(&html);
    let res = client
        .post(format!("{base}/admin/moments/{}/delete", m.id))
        .form(&[("csrf", csrf.as_str())])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302);
    let (_, total) = moments::list_moments(&pool, None, false, None, 1, 20).await.unwrap();
    assert_eq!(total, 0, "删除后说说应不存在");
    let html = client
        .get(format!("{base}/admin/moments"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(!html.contains(content), "删除后列表不应再含该内容");
    assert!(html.contains("暂无说说"), "空列表应显示占位文案");
}

#[tokio::test]
async fn admin_moments_list_shows_like_count_column() {
    let cfg = test_config("admin-moments-like-count");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    hancic::auth::set_password(&pool, common::TEST_PASSWORD)
        .await
        .unwrap();
    let (addr, client) = start_server_with_cfg(cfg).await;
    let base = format!("http://{addr}");
    assert!(login_admin(&client, &addr).await);

    let html = client.get(format!("{base}/admin/moments")).send().await.unwrap();
    let csrf = extract_csrf(&html.text().await.unwrap());
    let res = client
        .post(format!("{base}/admin/moments"))
        .form(&[
            ("content", "后台点赞说说"),
            ("attachment_ids", ""),
            ("csrf", csrf.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302);

    let (moments_list, total) = moments::list_moments(&pool, None, false, None, 1, 20).await.unwrap();
    assert_eq!(total, 1);
    let moment_id = moments_list[0].id;
    sqlx::query("UPDATE moments SET like_count = 4 WHERE id = ?")
        .bind(moment_id)
        .execute(&pool)
        .await
        .unwrap();

    let html = client
        .get(format!("{base}/admin/moments"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(html.contains("点赞数"), "说说列表应展示点赞数字段: {html}");
    assert!(html.contains(">4<") || html.contains("4"), "说说列表应展示点赞数 4: {html}");
}
