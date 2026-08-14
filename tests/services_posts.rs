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
