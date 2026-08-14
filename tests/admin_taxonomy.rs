//! T16：后台分类与标签管理集成测试。
//!
//! 覆盖：新建分类（slug 留空自动生成）/标签 → 列表含；slug 冲突错误回显
//! （`?msg=` + 错误提示）；删除分类后文章 category_id 置空（ON DELETE
//! SET NULL）；删除标签后 post_tags 关联清空（CASCADE），文章本身保留。

mod common;
use common::{extract_csrf, login_admin, start_server_with_cfg, test_config};
use hancic::db;
use hancic::models::{PostStatus, PostType};
use hancic::services::{posts, taxonomy};

/// 分类标签后台完整流程：建分类/标签 → 列表含 → slug 冲突错误回显 →
/// 更新分类 → 建文章归入分类 → 删分类置空 category_id → 删标签清空关联。
#[tokio::test]
async fn taxonomy_admin_flow() {
    let cfg = test_config("admin-taxonomy");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    hancic::auth::set_password(&pool, common::TEST_PASSWORD)
        .await
        .unwrap();
    let (addr, client) = start_server_with_cfg(cfg).await;
    let base = format!("http://{addr}");
    assert!(login_admin(&client, &addr).await);

    // 列表页可访问（顺带拿 CSRF），空列表有占位
    let res = client.get(format!("{base}/admin/taxonomy")).send().await.unwrap();
    assert_eq!(res.status(), 200, "分类标签页应可访问");
    let html = res.text().await.unwrap();
    let csrf = extract_csrf(&html);
    assert!(html.contains("暂无分类"), "空列表应有分类占位");
    assert!(html.contains("暂无标签"), "空列表应有标签占位");

    // 新建分类（slug 留空 → 从名称 slugify）与标签
    let res = client
        .post(format!("{base}/admin/taxonomy/categories"))
        .form(&[
            ("name", "技术 分享"),
            ("slug", ""),
            ("sort_order", "1"),
            ("csrf", csrf.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302, "新建分类应 302 回列表");
    let res = client
        .post(format!("{base}/admin/taxonomy/tags"))
        .form(&[("name", "Rust"), ("csrf", csrf.as_str())])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302, "新建标签应 302 回列表");

    // 服务层断言：分类 slug 从名称自动生成（CJK 保留、空白转 '-'），标签 slug 小写
    let cats = taxonomy::list_categories(&pool).await.unwrap();
    assert_eq!(cats.len(), 1);
    assert_eq!(cats[0].name, "技术 分享");
    assert_eq!(cats[0].slug, "技术-分享", "slug 留空应从名称 slugify");
    let tags = taxonomy::list_tags(&pool).await.unwrap();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].name, "Rust");
    assert_eq!(tags[0].slug, "rust");

    // 列表页含新分类与标签
    let res = client.get(format!("{base}/admin/taxonomy")).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let html = res.text().await.unwrap();
    assert!(html.contains("技术 分享"), "列表应含新分类");
    assert!(html.contains("Rust"), "列表应含新标签");
    let csrf = extract_csrf(&html);

    // slug 冲突 → 302 回列表带错误回显，冲突数据不入库
    let res = client
        .post(format!("{base}/admin/taxonomy/categories"))
        .form(&[
            ("name", "技术二"),
            ("slug", "技术-分享"),
            ("sort_order", "2"),
            ("csrf", csrf.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302, "slug 冲突应 302 回列表");
    // 客户端不跟随重定向：从 Location 取出 `?msg=` 后手动 GET 验证错误回显
    let location = res
        .headers()
        .get(reqwest::header::LOCATION)
        .expect("冲突应带回跳地址")
        .to_str()
        .unwrap()
        .to_string();
    assert!(location.contains("msg="), "冲突应带 msg 参数: {location}");
    let html = client
        .get(format!("{base}{location}"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(html.contains("slug 已存在"), "冲突提示应回显: {html}");
    assert_eq!(
        taxonomy::list_categories(&pool).await.unwrap().len(),
        1,
        "冲突分类不应入库"
    );

    // 更新分类：改名 + 改 slug + 排序
    let res = client
        .post(format!("{base}/admin/taxonomy/categories/{}/update", cats[0].id))
        .form(&[
            ("name", "编程"),
            ("slug", "code"),
            ("sort_order", "0"),
            ("csrf", csrf.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302, "更新分类应 302 回列表");
    let cats = taxonomy::list_categories(&pool).await.unwrap();
    assert_eq!(cats.len(), 1);
    assert_eq!(cats[0].name, "编程");
    assert_eq!(cats[0].slug, "code");

    // 建文章并归入该分类 + 打上标签，验证关联就位
    let cat = &cats[0];
    let post = posts::create_post(
        &pool,
        posts::NewPost {
            title: "测试文章".into(),
            content_md: "内容".into(),
            excerpt: None,
            slug: None,
            status: PostStatus::Draft,
            post_type: PostType::Post,
            category_id: Some(cat.id),
            column_id: None,
            tags: vec!["Rust".into()],
        },
    )
    .await
    .unwrap();
    assert_eq!(post.category_id, Some(cat.id));
    assert_eq!(posts::list_tags_of_post(&pool, post.id).await.unwrap().len(), 1);

    // 删除分类 → 文章 category_id 置空（ON DELETE SET NULL），文章不丢
    let res = client
        .post(format!("{base}/admin/taxonomy/categories/{}/delete", cat.id))
        .form(&[("csrf", csrf.as_str())])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302, "删除分类应 302 回列表");
    assert_eq!(taxonomy::list_categories(&pool).await.unwrap().len(), 0);
    let post = posts::get_post(&pool, post.id).await.unwrap().unwrap();
    assert_eq!(post.category_id, None, "删分类后文章 category_id 应置空");

    // 删除标签 → post_tags 关联清空（CASCADE），文章本身保留
    let res = client
        .post(format!("{base}/admin/taxonomy/tags/{}/delete", tags[0].id))
        .form(&[("csrf", csrf.as_str())])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302, "删除标签应 302 回列表");
    assert_eq!(taxonomy::list_tags(&pool).await.unwrap().len(), 0);
    assert_eq!(
        posts::list_tags_of_post(&pool, post.id).await.unwrap().len(),
        0,
        "删标签后 post_tags 关联应清空"
    );
    assert!(
        posts::get_post(&pool, post.id).await.unwrap().is_some(),
        "删除标签不应级联删文章"
    );
}
