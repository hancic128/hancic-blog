//! T13：后台文章管理集成测试。
//!
//! 覆盖：创建草稿 → 发布 → 编辑页含正文与 milkdown 资源 → 删除；
//! 自动保存（X-CSRF-Token 头）更新草稿正文且不改状态。

mod common;
use common::{extract_csrf, login_admin, start_server_with_cfg, test_config};
use hancic::db;
use hancic::models::PostStatus;
use hancic::services::posts;
use sqlx::Row;


/// 固定链接已改为系统生成的短 uuid，测试按标题反查文章 id。
async fn find_by_title(pool: &db::Db, title: &str) -> Option<hancic::models::Post> {
    let id: i64 = sqlx::query_scalar("SELECT id FROM posts WHERE title = ?")
        .bind(title)
        .fetch_optional(pool)
        .await
        .unwrap()?;
    posts::get_post(pool, id).await.unwrap()
}

/// 创建草稿（POST /admin/posts）→ 发布（POST /update）→ 编辑页含正文与
/// milkdown 资源（GET /edit）→ 删除（POST /delete）后按 slug 查无此文章。
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
    let p = find_by_title(&pool, "管理端文章").await.unwrap();
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
    let p = find_by_title(&pool, "管理端文章").await.unwrap();
    assert_eq!(p.status, PostStatus::Published, "更新后应为已发布");

    // 编辑页：含标题、正文、milkdown 资源与 _post
    let res = client
        .get(format!("{base}/admin/posts/{}/edit", p.id))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let html = res.text().await.unwrap();
    assert!(html.contains("管理端文章"), "编辑页应含标题");
    assert!(html.contains("# 正文"), "编辑页应含正文");
    assert!(html.contains("milkdown.min.js"), "编辑页应引 milkdown 编辑器");
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
        find_by_title(&pool, "管理端文章").await.is_none(),
        "删除后按标题不应查到"
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
    let p = find_by_title(&pool, "自动保存草稿").await.unwrap();

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

    let p = find_by_title(&pool, "自动保存草稿").await.unwrap();
    assert_eq!(p.content_md, "# 自动保存后的正文", "自动保存应更新正文");
    assert_eq!(p.status, PostStatus::Draft, "自动保存不应改变状态");
}

/// 固定链接由系统管理：创建自动生成 8 位短 uuid；编辑（不带 slug）不改变固定链接。
#[tokio::test]
async fn slug_is_managed_by_system() {
    let cfg = test_config("admin-posts-slug-managed");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    hancic::auth::set_password(&pool, common::TEST_PASSWORD)
        .await
        .unwrap();
    let (addr, client) = start_server_with_cfg(cfg).await;
    let base = format!("http://{addr}");
    assert!(login_admin(&client, &addr).await);
    let csrf = extract_csrf(
        &client
            .get(format!("{base}/admin/posts"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
    );

    // 创建（不提交 slug）→ 固定链接为 8 位短 uuid
    let res = client
        .post(format!("{base}/admin/posts"))
        .form(&[
            ("title", "甲文章"),
            ("content_md", "A 正文"),
            ("status", "draft"),
            ("category_id", ""),
            ("tags", ""),
            ("csrf", csrf.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302);
    let a = find_by_title(&pool, "甲文章").await.unwrap();
    assert_eq!(a.slug.len(), 8, "固定链接应为 8 位短 uuid: {}", a.slug);

    // 编辑：不带 slug 提交 → 固定链接保持不变
    let res = client
        .post(format!("{base}/admin/posts/{}/update", a.id))
        .form(&[
            ("title", "甲文章·新标题"),
            ("content_md", "# 新内容"),
            ("status", "draft"),
            ("category_id", ""),
            ("tags", "新标签"),
            ("csrf", csrf.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302, "编辑应正常重定向");
    let a2 = find_by_title(&pool, "甲文章·新标题").await.unwrap();
    assert_eq!(a2.slug, a.slug, "编辑不应改变固定链接");
    assert_eq!(a2.content_md, "# 新内容", "正文应更新");
}

#[tokio::test]
async fn admin_posts_list_shows_like_count_column() {
    let cfg = test_config("admin-posts-like-count");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    hancic::auth::set_password(&pool, common::TEST_PASSWORD)
        .await
        .unwrap();
    let (addr, client) = start_server_with_cfg(cfg).await;
    let base = format!("http://{addr}");
    assert!(login_admin(&client, &addr).await);

    let html = client.get(format!("{base}/admin/posts")).send().await.unwrap();
    let csrf = extract_csrf(&html.text().await.unwrap());
    let res = client
        .post(format!("{base}/admin/posts"))
        .form(&[
            ("title", "后台点赞文章"),
            ("content_md", "点赞正文"),
            ("status", "draft"),
            ("category_id", ""),
            ("tags", ""),
            ("csrf", csrf.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302);

    let post_id: i64 = sqlx::query("SELECT id FROM posts WHERE title = ?")
        .bind("后台点赞文章")
        .fetch_one(&pool)
        .await
        .unwrap()
        .get(0);
    sqlx::query("UPDATE posts SET like_count = 11 WHERE id = ?")
        .bind(post_id)
        .execute(&pool)
        .await
        .unwrap();

    let html = client
        .get(format!("{base}/admin/posts"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(html.contains("点赞数"), "列表页应展示点赞数字段: {html}");
    assert!(html.contains(">11<") || html.contains("11"), "列表页应展示点赞数 11: {html}");
}

#[tokio::test]
async fn admin_posts_list_sorts_by_like_count_desc() {
    let cfg = test_config("admin-posts-like-sort");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    hancic::auth::set_password(&pool, common::TEST_PASSWORD)
        .await
        .unwrap();
    let (addr, client) = start_server_with_cfg(cfg).await;
    let base = format!("http://{addr}");
    assert!(login_admin(&client, &addr).await);

    let html = client.get(format!("{base}/admin/posts")).send().await.unwrap();
    let csrf = extract_csrf(&html.text().await.unwrap());

    for title in ["高赞文章", "低赞文章"] {
        let res = client
            .post(format!("{base}/admin/posts"))
            .form(&[
                ("title", title),
                ("content_md", title),
                ("status", "draft"),
                ("category_id", ""),
                ("tags", ""),
                ("csrf", csrf.as_str()),
            ])
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 302);
    }

    let low_id: i64 = sqlx::query("SELECT id FROM posts WHERE title = ?")
        .bind("低赞文章")
        .fetch_one(&pool)
        .await
        .unwrap()
        .get(0);
    let high_id: i64 = sqlx::query("SELECT id FROM posts WHERE title = ?")
        .bind("高赞文章")
        .fetch_one(&pool)
        .await
        .unwrap()
        .get(0);
    sqlx::query("UPDATE posts SET like_count = 1 WHERE id = ?")
        .bind(low_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE posts SET like_count = 9 WHERE id = ?")
        .bind(high_id)
        .execute(&pool)
        .await
        .unwrap();

    let html = client
        .get(format!("{base}/admin/posts?sort=like_count&dir=desc"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let high_pos = html.find("高赞文章").expect("应包含高赞文章");
    let low_pos = html.find("低赞文章").expect("应包含低赞文章");
    assert!(high_pos < low_pos, "like_count desc 应先显示高赞文章: {html}");
}

/// 文章管理列表默认显示全部类型（文章 + 页面）；type=post 仅显示文章。
#[tokio::test]
async fn posts_list_defaults_to_all_types() {
    let cfg = test_config("admin-posts-all-types");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    hancic::auth::set_password(&pool, common::TEST_PASSWORD)
        .await
        .unwrap();
    let (addr, client) = start_server_with_cfg(cfg).await;
    let base = format!("http://{addr}");
    assert!(login_admin(&client, &addr).await);

    // seed：一篇文章 + 一个独立页面（直接写库，绕过 CSRF 流程）
    for (title, post_type) in [
        ("全部类型文章", "post"),
        ("全部类型页面", "page"),
    ] {
        sqlx::query(
            "INSERT INTO posts(slug, title, content_md, status, post_type, published_at)
             VALUES (?, ?, 'x', 'published', ?, strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        )
        .bind(format!("{post_type}-{title}"))
        .bind(title)
        .bind(post_type)
        .execute(&pool)
        .await
        .unwrap();
    }

    // 默认列表（无 type 参数）：文章与页面都应显示，类型筛选默认「全部类型」
    let html = client
        .get(format!("{base}/admin/posts"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(html.contains("全部类型文章"), "默认列表应含文章: {html}");
    assert!(html.contains("全部类型页面"), "默认列表应含页面: {html}");
    assert!(
        html.contains(r#"<option value="all" selected>全部类型</option>"#),
        "类型筛选应默认选中「全部类型」: {html}"
    );

    // type=post：仅文章
    let html = client
        .get(format!("{base}/admin/posts?type=post"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(html.contains("全部类型文章"), "type=post 应含文章: {html}");
    assert!(!html.contains("全部类型页面"), "type=post 不应含页面: {html}");
}
