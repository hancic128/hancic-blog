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
            category_id: None, tags: vec![],
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
            post_type: hancic::models::PostType::Post, category_id: None, tags: vec![],
        }).await.unwrap();
    }
    let (items, total) = posts::list_posts(&pool, PostListOptions {
        status: Some(PostStatus::Published), category_slug: None, tag_slug: None,
        page: 1, page_size: 2,
    }).await.unwrap();
    assert_eq!(total, 2);
    assert_eq!(items.len(), 2);
}

#[tokio::test]
async fn update_and_delete() {
    let (pool, _cfg) = setup("update-delete").await;
    let p = posts::create_post(&pool, NewPost {
        title: "旧标题".into(), content_md: "正文".into(), excerpt: None, slug: None,
        status: PostStatus::Draft, post_type: hancic::models::PostType::Post,
        category_id: None, tags: vec![],
    }).await.unwrap();
    let updated = posts::update_post(&pool, p.id, UpdatePost {
        title: Some("新标题".into()), content_md: None, excerpt: None, slug: None,
        status: Some(PostStatus::Published), post_type: None, category_id: None, tags: None,
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
        category_id: None, tags: vec!["a".into(), "b".into()],
    }).await.unwrap();
    let old = posts::list_tags_of_post(&pool, p.id).await.unwrap();
    assert_eq!(old.len(), 2);
    posts::update_post(&pool, p.id, UpdatePost {
        title: None, content_md: None, excerpt: None, slug: None,
        status: None, post_type: None, category_id: None, tags: Some(vec!["c".into()]),
    }).await.unwrap();
    let tags = posts::list_tags_of_post(&pool, p.id).await.unwrap();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].name, "c");
}

#[tokio::test]
async fn adjacent_posts_ordered_by_published_at() {
    let (pool, _cfg) = setup("adjacent").await;
    let mut ids = Vec::new();
    for i in 0..3 {
        let p = posts::create_post(&pool, NewPost {
            title: format!("相邻{i}"), content_md: "x".into(), excerpt: None, slug: None,
            status: PostStatus::Published, post_type: hancic::models::PostType::Post,
            category_id: None, tags: vec![],
        }).await.unwrap();
        ids.push(p.id);
    }
    let mid = posts::get_post(&pool, ids[1]).await.unwrap().unwrap();
    let (prev, next) = posts::adjacent_posts(&pool, &mid).await.unwrap();
    assert_eq!(prev.unwrap().id, ids[0]);
    assert_eq!(next.unwrap().id, ids[2]);
}
