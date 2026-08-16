mod common;

use common::test_config;
use hancic::db;
use hancic::models::{LikeContentType, PostStatus, PostType};
use hancic::services::{likes, moments, posts};
use hancic::services::posts::NewPost;
use std::sync::atomic::{AtomicU64, Ordering};

async fn setup_pool(tag: &str) -> sqlx::SqlitePool {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let suffix = SEQ.fetch_add(1, Ordering::Relaxed);
    let cfg = test_config(&format!("{tag}-{suffix}"));
    db::init(&cfg.data_dir).await.unwrap()
}

async fn setup_post() -> (sqlx::SqlitePool, i64) {
    let pool = setup_pool("likes-post").await;
    let post = posts::create_post(
        &pool,
        NewPost {
            title: "点赞文章".into(),
            content_md: "正文".into(),
            excerpt: None,
            slug: None,
            status: PostStatus::Published,
            post_type: PostType::Post,
            category_id: None,
            column_id: None,
            tags: vec![],
        },
    )
    .await
    .unwrap();
    (pool, post.id)
}

async fn setup_moment() -> (sqlx::SqlitePool, i64) {
    let pool = setup_pool("likes-moment").await;
    let moment = moments::create_moment(&pool, "点赞说说", &[]).await.unwrap();
    (pool, moment.id)
}

async fn create_post_with_like_count(pool: &sqlx::SqlitePool, title: &str, like_count: i64) -> i64 {
    let post = posts::create_post(
        pool,
        NewPost {
            title: title.into(),
            content_md: "正文".into(),
            excerpt: None,
            slug: None,
            status: PostStatus::Published,
            post_type: PostType::Post,
            category_id: None,
            column_id: None,
            tags: vec![],
        },
    )
    .await
    .unwrap();
    for i in 0..like_count {
        let visitor = format!("visitor-{title}-{i}");
        likes::toggle_like(
            pool,
            LikeContentType::Post,
            post.id,
            &visitor,
            "iphash",
            "uahash",
        )
        .await
        .unwrap();
    }
    post.id
}

#[tokio::test]
async fn toggle_like_adds_then_removes_post_like() {
    let (pool, post_id) = setup_post().await;

    let liked = likes::toggle_like(
        &pool,
        LikeContentType::Post,
        post_id,
        "visitor-a",
        "iphash",
        "uahash",
    )
    .await
    .unwrap();
    assert!(liked.liked);
    assert_eq!(liked.like_count, 1);

    let unliked = likes::toggle_like(
        &pool,
        LikeContentType::Post,
        post_id,
        "visitor-a",
        "iphash",
        "uahash",
    )
    .await
    .unwrap();
    assert!(!unliked.liked);
    assert_eq!(unliked.like_count, 0);
}

#[tokio::test]
async fn toggle_like_adds_then_removes_moment_like() {
    let (pool, moment_id) = setup_moment().await;

    let liked = likes::toggle_like(
        &pool,
        LikeContentType::Moment,
        moment_id,
        "visitor-b",
        "iphash",
        "uahash",
    )
    .await
    .unwrap();
    assert!(liked.liked);
    assert_eq!(liked.like_count, 1);

    let unliked = likes::toggle_like(
        &pool,
        LikeContentType::Moment,
        moment_id,
        "visitor-b",
        "iphash",
        "uahash",
    )
    .await
    .unwrap();
    assert!(!unliked.liked);
    assert_eq!(unliked.like_count, 0);
}

#[tokio::test]
async fn list_posts_can_sort_by_like_count_desc() {
    let pool = setup_pool("likes-sort").await;
    let high = create_post_with_like_count(&pool, "高赞", 3).await;
    let low = create_post_with_like_count(&pool, "低赞", 1).await;

    let (items, _) = posts::list_posts(
        &pool,
        posts::PostListOptions {
            status: Some(PostStatus::Published),
            post_type: Some(PostType::Post),
            category_slug: None,
            tag_slug: None,
            column_slug: None,
            month: None,
            sort: Some(posts::PostSort { field: "like_count", asc: false }),
            page: 1,
            page_size: 10,
        },
    )
    .await
    .unwrap();

    assert_eq!(items[0].id, high);
    assert_eq!(items[1].id, low);
}

#[tokio::test]
async fn like_status_reports_liked_and_count_for_post() {
    let (pool, post_id) = setup_post().await;

    let before = likes::like_status(&pool, LikeContentType::Post, post_id, "visitor-c")
        .await
        .unwrap();
    assert!(!before.liked);
    assert_eq!(before.like_count, 0);

    likes::toggle_like(
        &pool,
        LikeContentType::Post,
        post_id,
        "visitor-c",
        "iphash",
        "uahash",
    )
    .await
    .unwrap();

    let after = likes::like_status(&pool, LikeContentType::Post, post_id, "visitor-c")
        .await
        .unwrap();
    assert!(after.liked);
    assert_eq!(after.like_count, 1);
}

#[tokio::test]
async fn recent_like_count_counts_rows_in_window() {
    let (pool, post_id) = setup_post().await;
    likes::toggle_like(
        &pool,
        LikeContentType::Post,
        post_id,
        "visitor-d",
        "iphash",
        "uahash",
    )
    .await
    .unwrap();
    likes::toggle_like(
        &pool,
        LikeContentType::Post,
        post_id,
        "visitor-e",
        "iphash",
        "uahash",
    )
    .await
    .unwrap();

    let total = likes::recent_like_count(&pool, 7).await.unwrap();
    assert_eq!(total, 2);
}

#[tokio::test]
async fn duplicate_like_row_recovery_keeps_existing_visitor_toggle_semantics() {
    let (pool, post_id) = setup_post().await;
    sqlx::query(
        "INSERT INTO content_likes(content_type, content_id, visitor_id, ip_hash, ua_hash) VALUES (?, ?, ?, ?, ?)",
    )
    .bind("post")
    .bind(post_id)
    .bind("visitor-race")
    .bind("iphash")
    .bind("uahash")
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query("UPDATE posts SET like_count = 0 WHERE id = ?")
        .bind(post_id)
        .execute(&pool)
        .await
        .unwrap();

    let status = likes::toggle_like(
        &pool,
        LikeContentType::Post,
        post_id,
        "visitor-race",
        "iphash",
        "uahash",
    )
    .await;

    assert!(status.is_ok(), "recovery path must not surface internal error");
    let status = status.unwrap();
    assert!(!status.liked);
    assert_eq!(status.like_count, 0);

    let rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM content_likes WHERE content_type = ? AND content_id = ? AND visitor_id = ?",
    )
    .bind("post")
    .bind(post_id)
    .bind("visitor-race")
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(rows, 0);
}

#[test]
fn hash_client_hint_returns_expected_sha256_hex() {
    let hashed = likes::hash_client_hint("127.0.0.1");
    assert_eq!(hashed.len(), 64);
    assert_eq!(hashed, "12ca17b49af2289436f303e0166030a21e525d266e209267433801a8fd4071a0");
    assert_ne!(hashed, likes::hash_client_hint("127.0.0.2"));
}
