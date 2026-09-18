mod common;
use common::test_config;
use hancic::db;
use hancic::models::PostStatus;
use hancic::services::posts::{self, NewPost, PostListOptions, UpdatePost};

async fn setup(tag: &str) -> (sqlx::SqlitePool, hancic::config::Config) {
    let cfg = test_config(tag);
    let pool = db::init(&cfg.data_dir).await.unwrap();
    (pool, cfg)
}

#[tokio::test]
async fn create_and_get_post() {
    let (pool, _cfg) = setup("create-post").await;
    let p = posts::create_post(&pool, NewPost {
        title: "Hello 世界".into(), content_md: "# 标题\n正文".into(),
        excerpt: None, slug: None, status: PostStatus::Published,
        post_type: hancic::models::PostType::Post, category_id: None,
        column_id: None,
        tags: vec!["rust".into(), "博客".into()],
    }).await.unwrap();
    assert_eq!(p.slug, "hello-世界");
    assert_eq!(p.status, PostStatus::Published);
    let fetched = posts::get_post_by_slug(&pool, "hello-世界").await.unwrap().unwrap();
    assert_eq!(fetched.title, "Hello 世界");
    let tags = posts::list_tags_of_post(&pool, p.id).await.unwrap();
    assert_eq!(tags.len(), 2);
}

#[tokio::test]
async fn slug_conflict_appends_suffix() {
    let (pool, _cfg) = setup("slug-conflict").await;
    for _ in 1..=2 {
        posts::create_post(&pool, NewPost {
            title: "同一标题".into(), content_md: "x".into(), excerpt: None, slug: None,
            status: PostStatus::Draft, post_type: hancic::models::PostType::Post,
            category_id: None, column_id: None, tags: vec![],
        }).await.unwrap();
    }
    let a = posts::get_post_by_slug(&pool, "同一标题").await.unwrap().unwrap();
    let b = posts::get_post_by_slug(&pool, "同一标题-2").await.unwrap().unwrap();
    assert_ne!(a.id, b.id);
}

#[tokio::test]
async fn list_published_only_and_paginate() {
    let (pool, _cfg) = setup("list-posts").await;
    for i in 0..3 {
        posts::create_post(&pool, NewPost {
            title: format!("文章{i}"), content_md: "x".into(), excerpt: None, slug: None,
            status: if i == 2 { PostStatus::Draft } else { PostStatus::Published },
            post_type: hancic::models::PostType::Post, category_id: None, column_id: None, tags: vec![],
        }).await.unwrap();
    }
    let (items, total) = posts::list_posts(&pool, PostListOptions {
        status: Some(PostStatus::Published), post_type: None,
        category_slug: None, tag_slug: None, column_slug: None, month: None, sort: None,
        page: 1, page_size: 2,
    }).await.unwrap();
    assert_eq!(total, 2);
    assert_eq!(items.len(), 2);
}

/// I4：create 传纯标点 slug（slugify 后为空，如 `---`）→ 回退标题生成 slug，
/// 不写入空 slug。
#[tokio::test]
async fn create_punctuation_slug_falls_back_to_title() {
    let (pool, _cfg) = setup("slug-punct-create").await;
    let p = posts::create_post(&pool, NewPost {
        title: "Hello 世界".into(), content_md: "x".into(), excerpt: None,
        slug: Some("---".into()), status: PostStatus::Draft,
        post_type: hancic::models::PostType::Post, category_id: None, column_id: None, tags: vec![],
    }).await.unwrap();
    assert_eq!(p.slug, "hello-世界", "纯标点 slug 应回退标题");
    assert!(!p.slug.is_empty(), "slug 不得为空");
}

/// I4：update 传纯标点 slug（`---`）→ 视为不变，保留原 slug。
#[tokio::test]
async fn update_punctuation_slug_keeps_original() {
    let (pool, _cfg) = setup("slug-punct-update").await;
    let p = posts::create_post(&pool, NewPost {
        title: "原标题".into(), content_md: "x".into(), excerpt: None,
        slug: Some("original-slug".into()), status: PostStatus::Draft,
        post_type: hancic::models::PostType::Post, category_id: None, column_id: None, tags: vec![],
    }).await.unwrap();
    let updated = posts::update_post(&pool, p.id, UpdatePost {
        title: Some("新标题".into()), content_md: None, excerpt: None,
        slug: Some("---".into()), status: None, post_type: None, category_id: None, column_id: None, tags: None,
        published_at: None, updated_at: None,
    }).await.unwrap();
    assert_eq!(updated.slug, "original-slug", "纯标点 slug 应视为不变");
    assert_eq!(updated.title, "新标题", "其余字段更新不受影响");
}

#[tokio::test]
async fn update_and_delete() {
    let (pool, _cfg) = setup("update-delete").await;
    let p = posts::create_post(&pool, NewPost {
        title: "旧标题".into(), content_md: "正文".into(), excerpt: None, slug: None,
        status: PostStatus::Draft, post_type: hancic::models::PostType::Post,
        category_id: None, column_id: None, tags: vec![],
    }).await.unwrap();
    let updated = posts::update_post(&pool, p.id, UpdatePost {
        title: Some("新标题".into()), content_md: None, excerpt: None, slug: None,
        status: Some(PostStatus::Published), post_type: None, category_id: None, column_id: None, tags: None,
        published_at: None, updated_at: None,
    }).await.unwrap();
    assert_eq!(updated.title, "新标题");
    assert_eq!(updated.status, PostStatus::Published);
    assert!(updated.published_at.is_some());
    posts::delete_post(&pool, p.id).await.unwrap();
    assert!(posts::get_post(&pool, p.id).await.unwrap().is_none());
}

#[tokio::test]
async fn update_replaces_tags() {
    let (pool, _cfg) = setup("update-tags").await;
    let p = posts::create_post(&pool, NewPost {
        title: "标签替换".into(), content_md: "x".into(), excerpt: None, slug: None,
        status: PostStatus::Draft, post_type: hancic::models::PostType::Post,
        category_id: None, column_id: None, tags: vec!["a".into(), "b".into()],
    }).await.unwrap();
    let old = posts::list_tags_of_post(&pool, p.id).await.unwrap();
    assert_eq!(old.len(), 2);
    posts::update_post(&pool, p.id, UpdatePost {
        title: None, content_md: None, excerpt: None, slug: None,
        status: None, post_type: None, category_id: None, column_id: None, tags: Some(vec!["c".into()]),
        published_at: None, updated_at: None,
    }).await.unwrap();
    let tags = posts::list_tags_of_post(&pool, p.id).await.unwrap();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].name, "c");
}

/// `UpdatePost.category_id = Some(None)`：显式清空分类（SET NULL）。
#[tokio::test]
async fn update_clears_category() {
    let (pool, _cfg) = setup("update-clear-category").await;
    let cat = hancic::services::taxonomy::create_category(&pool, "随笔", "notes", 0)
        .await
        .unwrap();
    let p = posts::create_post(&pool, NewPost {
        title: "分类清空".into(), content_md: "x".into(), excerpt: None, slug: None,
        status: PostStatus::Draft, post_type: hancic::models::PostType::Post,
        category_id: Some(cat.id), tags: vec![],
        column_id: None,
    }).await.unwrap();
    assert_eq!(p.category_id, Some(cat.id), "前置：已设分类");
    let updated = posts::update_post(&pool, p.id, UpdatePost {
        title: None, content_md: None, excerpt: None, slug: None,
        status: None, post_type: None, category_id: Some(None), tags: None,
        column_id: None,
        published_at: None, updated_at: None,
    }).await.unwrap();
    assert_eq!(updated.category_id, None, "分类应被清空为 NULL");
}

/// `UpdatePost.excerpt = Some(None)`：显式清空摘要（空串）。
#[tokio::test]
async fn update_clears_excerpt() {
    let (pool, _cfg) = setup("update-clear-excerpt").await;
    let p = posts::create_post(&pool, NewPost {
        title: "摘要清空".into(), content_md: "x".into(), excerpt: Some("旧摘要".into()), slug: None,
        status: PostStatus::Draft, post_type: hancic::models::PostType::Post,
        category_id: None, column_id: None, tags: vec![],
    }).await.unwrap();
    assert_eq!(p.excerpt, "旧摘要", "前置：已有摘要");
    let updated = posts::update_post(&pool, p.id, UpdatePost {
        title: None, content_md: None, excerpt: Some(None), slug: None,
        status: None, post_type: None, category_id: None, column_id: None, tags: None,
        published_at: None, updated_at: None,
    }).await.unwrap();
    assert_eq!(updated.excerpt, "", "摘要应被清空");
}

#[tokio::test]
async fn adjacent_posts_ordered_by_published_at() {
    let (pool, _cfg) = setup("adjacent").await;
    let mut ids = Vec::new();
    for i in 0..3 {
        let p = posts::create_post(&pool, NewPost {
            title: format!("相邻{i}"), content_md: "x".into(), excerpt: None, slug: None,
            status: PostStatus::Published, post_type: hancic::models::PostType::Post,
            category_id: None, column_id: None, tags: vec![],
        }).await.unwrap();
        ids.push(p.id);
    }
    let mid = posts::get_post(&pool, ids[1]).await.unwrap().unwrap();
    let (prev, next) = posts::adjacent_posts(&pool, &mid).await.unwrap();
    assert_eq!(prev.unwrap().id, ids[0]);
    assert_eq!(next.unwrap().id, ids[2]);
}

#[tokio::test]
async fn adjacent_posts_skip_pages() {
    let (pool, _cfg) = setup("adjacent-page").await;
    use std::time::Duration;
    // 时间序：post A < page P < post B；P 不能成为 A/B 的相邻文章
    let a = posts::create_post(&pool, NewPost {
        title: "前文".into(), content_md: "x".into(), excerpt: None, slug: None,
        status: PostStatus::Published, post_type: hancic::models::PostType::Post,
        category_id: None, column_id: None, tags: vec![],
    }).await.unwrap();
    tokio::time::sleep(Duration::from_millis(5)).await;
    let p = posts::create_post(&pool, NewPost {
        title: "中间页".into(), content_md: "x".into(), excerpt: None, slug: Some("mid-page".into()),
        status: PostStatus::Published, post_type: hancic::models::PostType::Page,
        category_id: None, column_id: None, tags: vec![],
    }).await.unwrap();
    tokio::time::sleep(Duration::from_millis(5)).await;
    let b = posts::create_post(&pool, NewPost {
        title: "后文".into(), content_md: "x".into(), excerpt: None, slug: None,
        status: PostStatus::Published, post_type: hancic::models::PostType::Post,
        category_id: None, column_id: None, tags: vec![],
    }).await.unwrap();

    let (prev, next) = posts::adjacent_posts(&pool, &a).await.unwrap();
    assert!(prev.is_none());
    assert_eq!(next.unwrap().id, b.id);

    let (prev, next) = posts::adjacent_posts(&pool, &b).await.unwrap();
    assert_eq!(prev.unwrap().id, a.id);
    assert!(next.is_none());

    // 相邻结果不得出现 page
    let (prev, next) = posts::adjacent_posts(&pool, &a).await.unwrap();
    assert_ne!(prev.as_ref().map(|x| x.id), Some(p.id));
    assert_ne!(next.as_ref().map(|x| x.id), Some(p.id));
}

#[tokio::test]
async fn heatmap_counts_only_published() {
    let (pool, _cfg) = setup("heatmap").await;
    posts::create_post(&pool, NewPost {
        title: "已发布A".into(), content_md: "x".into(), excerpt: None, slug: None,
        status: PostStatus::Published, post_type: hancic::models::PostType::Post,
        category_id: None, column_id: None, tags: vec![],
    }).await.unwrap();
    posts::create_post(&pool, NewPost {
        title: "已发布B".into(), content_md: "x".into(), excerpt: None, slug: None,
        status: PostStatus::Published, post_type: hancic::models::PostType::Post,
        category_id: None, column_id: None, tags: vec![],
    }).await.unwrap();
    posts::create_post(&pool, NewPost {
        title: "草稿C".into(), content_md: "x".into(), excerpt: None, slug: None,
        status: PostStatus::Draft, post_type: hancic::models::PostType::Post,
        category_id: None, column_id: None, tags: vec![],
    }).await.unwrap();
    let h = posts::heatmap(&pool, 7).await.unwrap();
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    assert!(h.iter().any(|(d, c)| *d == today && *c == 2), "当日应计 2 篇（草稿不计），实际 {h:?}");
}

/// P1-G 回归保险：按 category 筛选 + 分页,total 应与 items 总数一致,
/// content_md 不拼接(每条只对应自己的 content)。
#[tokio::test]
async fn list_paginate_by_category_keeps_items_total_consistent() {
    use hancic::services::taxonomy;
    let (pool, _cfg) = setup("list-cat-paginate").await;
    let cat = taxonomy::create_category(&pool, "科技", "tech", 0).await.unwrap();
    let other = taxonomy::create_category(&pool, "生活", "life", 0).await.unwrap();
    for i in 0..5 {
        let (cid, marker) = if i < 3 { (cat.id, format!("cat1-marker-{i}")) } else { (other.id, format!("cat2-marker-{i}")) };
        posts::create_post(&pool, NewPost {
            title: format!("科技文{i}"), content_md: marker.clone(), excerpt: None,
            slug: None, status: PostStatus::Published,
            post_type: hancic::models::PostType::Post, category_id: Some(cid),
            column_id: None, tags: vec![],
        }).await.unwrap();
    }
    // page=1 page_size=2:cat1 总数=3,本页 2 条
    let (p1, t1) = posts::list_posts(&pool, PostListOptions {
        status: Some(PostStatus::Published), post_type: Some(hancic::models::PostType::Post),
        category_slug: Some("tech".into()), tag_slug: None, column_slug: None, month: None, sort: None,
        page: 1, page_size: 2,
    }).await.unwrap();
    assert_eq!(t1, 3, "cat1 总数应为 3");
    assert_eq!(p1.len(), 2, "page=1 应返 2 条");
    assert!(p1.iter().all(|p| p.content_md.starts_with("cat1-marker-")), "不应混入 cat2 内容,实际 {:?}",
        p1.iter().map(|p| (&p.title, &p.content_md)).collect::<Vec<_>>());
    // page=2 page_size=2:cat1 总数=3,本页 1 条
    let (p2, t2) = posts::list_posts(&pool, PostListOptions {
        status: Some(PostStatus::Published), post_type: Some(hancic::models::PostType::Post),
        category_slug: Some("tech".into()), tag_slug: None, column_slug: None, month: None, sort: None,
        page: 2, page_size: 2,
    }).await.unwrap();
    assert_eq!(t2, 3, "cat1 总数 page=2 应仍为 3");
    assert_eq!(p2.len(), 1, "page=2 应返 1 条");
    assert!(p2[0].content_md.starts_with("cat1-marker-"), "page=2 也只返 cat1 内容");
    // cat2 总数=2,不应受 cat1 干扰
    let (p3, t3) = posts::list_posts(&pool, PostListOptions {
        status: Some(PostStatus::Published), post_type: Some(hancic::models::PostType::Post),
        category_slug: Some("life".into()), tag_slug: None, column_slug: None, month: None, sort: None,
        page: 1, page_size: 10,
    }).await.unwrap();
    assert_eq!(t3, 2);
    assert_eq!(p3.len(), 2);
    assert!(p3.iter().all(|p| p.content_md.starts_with("cat2-marker-")));
}

/// P1-H：草稿 → 发布 时显式传 `published_at`，时间戳为用户值而非 Utc::now()。
#[tokio::test]
async fn update_with_custom_published_at() {
    let (pool, _cfg) = setup("update-custom-pub").await;
    let p = posts::create_post(&pool, NewPost {
        title: "回填测试".into(), content_md: "x".into(), excerpt: None, slug: None,
        status: PostStatus::Draft, post_type: hancic::models::PostType::Post,
        category_id: None, column_id: None, tags: vec![],
    }).await.unwrap();
    let custom: chrono::DateTime<chrono::Utc> = "2026-09-15T01:30:00Z".parse().unwrap();
    let updated = posts::update_post(&pool, p.id, UpdatePost {
        title: None, content_md: None, excerpt: None, slug: None,
        status: Some(PostStatus::Published), post_type: None,
        category_id: None, column_id: None, tags: None,
        published_at: Some(Some(custom)),
        updated_at: None,
    }).await.unwrap();
    assert_eq!(
        updated.published_at.unwrap().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true),
        custom.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true),
        "published_at 应为用户值"
    );
}

/// P1-H：传 `updated_at` 时跳过自动刷新，DB 里的 updated_at 等于用户值。
#[tokio::test]
async fn update_with_custom_updated_at_skips_auto_flush() {
    let (pool, _cfg) = setup("update-custom-upd").await;
    let p = posts::create_post(&pool, NewPost {
        title: "回填 updated".into(), content_md: "old".into(), excerpt: None, slug: None,
        status: PostStatus::Draft, post_type: hancic::models::PostType::Post,
        category_id: None, column_id: None, tags: vec![],
    }).await.unwrap();
    let custom: chrono::DateTime<chrono::Utc> = "2026-09-15T01:30:00Z".parse().unwrap();
    let updated = posts::update_post(&pool, p.id, UpdatePost {
        title: None, content_md: Some("new".into()), excerpt: None, slug: None,
        status: None, post_type: None, category_id: None, column_id: None, tags: None,
        published_at: None,
        updated_at: Some(custom),
    }).await.unwrap();
    assert_eq!(
        updated.updated_at.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true),
        custom.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true),
        "updated_at 应跳过自动刷,等于用户值"
    );
}

/// P1-H：不传 `updated_at` 时仍自动刷 Utc::now()（回归保险）。
#[tokio::test]
async fn update_without_updated_at_auto_flushes() {
    let (pool, _cfg) = setup("update-auto-upd").await;
    let p = posts::create_post(&pool, NewPost {
        title: "自动刷".into(), content_md: "old".into(), excerpt: None, slug: None,
        status: PostStatus::Draft, post_type: hancic::models::PostType::Post,
        category_id: None, column_id: None, tags: vec![],
    }).await.unwrap();
    let before = p.updated_at;
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let updated = posts::update_post(&pool, p.id, UpdatePost {
        title: None, content_md: Some("new".into()), excerpt: None, slug: None,
        status: None, post_type: None, category_id: None, column_id: None, tags: None,
        published_at: None,
        updated_at: None,
    }).await.unwrap();
    assert!(updated.updated_at > before, "未传 updated_at 时仍应自动刷");
}

/// P1-H：专用 service `update_post_timestamps`，仅改时间字段。
#[tokio::test]
async fn update_post_timestamps_only() {
    let (pool, _cfg) = setup("update-ts-only").await;
    let p = posts::create_post(&pool, NewPost {
        title: "仅改时间".into(), content_md: "不变".into(), excerpt: None, slug: None,
        status: PostStatus::Draft, post_type: hancic::models::PostType::Post,
        category_id: None, column_id: None, tags: vec![],
    }).await.unwrap();
    let custom_pub: chrono::DateTime<chrono::Utc> = "2026-09-14T23:00:00Z".parse().unwrap();
    let custom_upd: chrono::DateTime<chrono::Utc> = "2026-09-15T01:30:00Z".parse().unwrap();
    let updated = posts::update_post_timestamps(
        &pool, p.id,
        Some(Some(custom_pub)),
        Some(custom_upd),
    ).await.unwrap();
    assert_eq!(updated.content_md, "不变", "正文不应被时间戳端点动");
    assert_eq!(updated.title, "仅改时间", "标题不应被时间戳端点动");
    assert_eq!(
        updated.published_at.unwrap().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true),
        custom_pub.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true),
    );
    assert_eq!(
        updated.updated_at.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true),
        custom_upd.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true),
    );
    // published_at 清空为 None
    let cleared = posts::update_post_timestamps(
        &pool, p.id, Some(None), None,
    ).await.unwrap();
    assert!(cleared.published_at.is_none(), "传 None 应清空 published_at");
    // 两参数都 None：不写库，返回当前 Post
    let noop = posts::update_post_timestamps(&pool, p.id, None, None).await.unwrap();
    assert_eq!(noop.id, p.id);
}

#[tokio::test]
async fn recent_activity_merges_posts_and_moments() {
    let (pool, _cfg) = setup("activity").await;
    posts::create_post(&pool, NewPost {
        title: "活动文章".into(), content_md: "x".into(), excerpt: None, slug: None,
        status: PostStatus::Published, post_type: hancic::models::PostType::Post,
        category_id: None, column_id: None, tags: vec![],
    }).await.unwrap();
    hancic::services::moments::create_moment(&pool, "活动说说", &[]).await.unwrap();
    let acts = posts::recent_activity(&pool, 7, 10).await.unwrap();
    assert_eq!(acts.len(), 2);
    assert!(acts.iter().any(|a| a.kind == "post" && a.title == "活动文章"));
    assert!(acts.iter().any(|a| a.kind == "moment" && a.title.starts_with("活动说说")));
}
