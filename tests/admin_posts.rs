//! T13：后台文章管理集成测试。
//!
//! 覆盖：创建草稿 → 发布 → 编辑页含正文与 Vditor 资源 → 删除；
//! 自动保存（X-CSRF-Token 头）更新草稿正文且不改状态。

mod common;
use common::{extract_csrf, login_admin, start_server_with_cfg, test_config};
use hancic::db;
use hancic::models::PostStatus;
use hancic::services::posts;

/// 创建草稿（POST /admin/posts）→ 发布（POST /update）→ 编辑页含正文与
/// Vditor 资源（GET /edit）→ 删除（POST /delete）后按 slug 查无此文章。
#[tokio::test]
async fn create_publish_edit_delete_flow() {
    let cfg = test_config("admin-posts");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    hancic::auth::set_password(&pool, common::TEST_PASSWORD)
        .await
        .unwrap();
    let (addr, client) = start_server_with_cfg(cfg).await;
    let base = format!("http://{addr}");
    assert!(login_admin(&client, &addr).await);

    // 列表页拿 CSRF（顺带验证列表页可访问）
    let res = client.get(format!("{base}/admin/posts")).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let html = res.text().await.unwrap();
    let csrf = extract_csrf(&html);

    // 创建草稿
    let res = client
        .post(format!("{base}/admin/posts"))
        .form(&[
            ("title", "管理端文章"),
            ("content_md", "# 正文\n内容"),
            ("status", "draft"),
            ("category_id", ""),
            ("tags", ""),
            ("csrf", csrf.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302);
    let p = posts::get_post_by_slug(&pool, "管理端文章")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(p.status, PostStatus::Draft, "创建后应为草稿");
    assert_eq!(p.content_md, "# 正文\n内容", "正文应原样落库");

    // 编辑页拿 CSRF 并发布
    let html = client
        .get(format!("{base}/admin/posts/{}/edit", p.id))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let csrf = extract_csrf(&html);
    let res = client
        .post(format!("{base}/admin/posts/{}/update", p.id))
        .form(&[
            ("title", "管理端文章"),
            ("content_md", "# 正文\n内容"),
            ("status", "published"),
            ("category_id", ""),
            ("tags", ""),
            ("csrf", csrf.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302);
    let p = posts::get_post_by_slug(&pool, "管理端文章")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(p.status, PostStatus::Published, "更新后应为已发布");

    // 编辑页：含标题、正文、Vditor 资源与 _post
    let res = client
        .get(format!("{base}/admin/posts/{}/edit", p.id))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let html = res.text().await.unwrap();
    assert!(html.contains("管理端文章"), "编辑页应含标题");
    assert!(html.contains("# 正文"), "编辑页应含正文");
    assert!(html.contains("vditor.min.js"), "编辑页应引 Vditor");
    assert!(html.contains("window._post"), "编辑页应输出 _post");

    // 删除 → 按 slug 查无
    let csrf = extract_csrf(&html);
    let res = client
        .post(format!("{base}/admin/posts/{}/delete", p.id))
        .form(&[("csrf", csrf.as_str())])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302);
    assert!(
        posts::get_post_by_slug(&pool, "管理端文章")
            .await
            .unwrap()
            .is_none(),
        "删除后文章应不存在"
    );
}

/// 自动保存：POST /admin/posts/{id}/autosave 带 X-CSRF-Token 头 + content_md，
/// 只更新正文，不改 status。
#[tokio::test]
async fn autosave_updates_draft_content() {
    let cfg = test_config("admin-posts-autosave");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    hancic::auth::set_password(&pool, common::TEST_PASSWORD)
        .await
        .unwrap();
    let (addr, client) = start_server_with_cfg(cfg).await;
    let base = format!("http://{addr}");
    assert!(login_admin(&client, &addr).await);

    // 建一篇草稿
    let html = client.get(format!("{base}/admin/posts")).send().await.unwrap();
    let csrf = extract_csrf(&html.text().await.unwrap());
    let res = client
        .post(format!("{base}/admin/posts"))
        .form(&[
            ("title", "自动保存草稿"),
            ("content_md", "初始内容"),
            ("status", "draft"),
            ("category_id", ""),
            ("tags", ""),
            ("csrf", csrf.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302);
    let p = posts::get_post_by_slug(&pool, "自动保存草稿")
        .await
        .unwrap()
        .unwrap();

    // 自动保存：正文更新，状态保持草稿
    let res = client
        .post(format!("{base}/admin/posts/{}/autosave", p.id))
        .header("X-CSRF-Token", &csrf)
        .form(&[("content_md", "# 自动保存后的正文")])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "自动保存应 200");
    let json: serde_json::Value = res.json().await.unwrap();
    assert_eq!(json["data"]["ok"], true, "应返回 ok: {json}");

    let p = posts::get_post_by_slug(&pool, "自动保存草稿")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(p.content_md, "# 自动保存后的正文", "自动保存应更新正文");
    assert_eq!(p.status, PostStatus::Draft, "自动保存不应改变状态");
}
