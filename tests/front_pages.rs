//! 前台页面集成测试：首页、文章页、404、分类页、关于页。

mod common;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::test_app;
use hancic::db;
use hancic::models::{PostStatus, PostType};
use hancic::services::posts::{self, NewPost};
use hancic::services::taxonomy;
use tower::ServiceExt;

/// 便捷：创建一篇已发布文章。
async fn create_published_post(
    pool: &db::Db,
    title: &str,
    category_id: Option<i64>,
    tags: Vec<String>,
) -> i64 {
    posts::create_post(
        pool,
        NewPost {
            title: title.into(),
            content_md: "# 标题\n\n正文内容 **加粗**".into(),
            excerpt: None,
            slug: None,
            status: PostStatus::Published,
            post_type: PostType::Post,
            category_id,
            tags,
        },
    )
    .await
    .unwrap()
    .id
}

async fn get_html(app: &axum::Router, uri: &str) -> (StatusCode, String) {
    let res = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    (status, String::from_utf8(bytes).unwrap())
}

#[tokio::test]
async fn homepage_lists_published_posts() {
    let (app, pool) = test_app("front-home").await;
    create_published_post(&pool, "第一篇文章", None, vec!["rust".into()]).await;

    let (status, html) = get_html(&app, "/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("第一篇文章"));
    assert!(html.contains("寒蝉 Hancic"));
}

#[tokio::test]
async fn post_page_renders_markdown() {
    let (app, pool) = test_app("front-post").await;
    create_published_post(&pool, "第一篇文章", None, vec!["rust".into()]).await;

    // slug 由标题生成：slugify 保留 CJK，故为「第一篇文章」
    let (status, html) = get_html(&app, "/post/第一篇文章").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<h1"));
    assert!(html.contains("正文内容"));
}

#[tokio::test]
async fn unknown_slug_404() {
    let (app, _pool) = test_app("front-404").await;
    let (status, _html) = get_html(&app, "/post/不存在的文章").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn category_page_filters() {
    let (app, pool) = test_app("front-category").await;
    let cat = taxonomy::create_category(&pool, "技术", "tech", 0).await.unwrap();
    create_published_post(&pool, "Rust 入门", Some(cat.id), vec![]).await;
    create_published_post(&pool, "无关文章", None, vec![]).await;

    let (status, html) = get_html(&app, "/category/tech").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Rust 入门"));
    assert!(!html.contains("无关文章"));
}

#[tokio::test]
async fn homepage_excludes_pages() {
    let (app, pool) = test_app("front-home-no-page").await;
    create_published_post(&pool, "普通文章", None, vec![]).await;
    posts::create_post(
        &pool,
        NewPost {
            title: "独立页面".into(),
            content_md: "x".into(),
            excerpt: None,
            slug: Some("standalone".into()),
            status: PostStatus::Published,
            post_type: PostType::Page,
            category_id: None,
            tags: vec![],
        },
    )
    .await
    .unwrap();

    let (status, html) = get_html(&app, "/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("普通文章"));
    assert!(!html.contains("独立页面"));
}

#[tokio::test]
async fn about_page_renders_page_type() {
    let (app, pool) = test_app("front-about").await;
    posts::create_post(
        &pool,
        NewPost {
            title: "关于本站".into(),
            content_md: "## 站点介绍\n\n这是一个博客。".into(),
            excerpt: None,
            slug: Some("about".into()),
            status: PostStatus::Published,
            post_type: PostType::Page,
            category_id: None,
            tags: vec![],
        },
    )
    .await
    .unwrap();

    let (status, html) = get_html(&app, "/about").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("关于本站"));
    assert!(html.contains("<h2"));
}

#[tokio::test]
async fn homepage_has_heatmap_activity_and_more_link() {
    let (app, pool) = test_app("front-home-aggregate").await;
    for i in 1..=6 {
        posts::create_post(&pool, NewPost {
            title: format!("聚合页文章{i}"), content_md: "内容".into(), excerpt: None, slug: None,
            status: PostStatus::Published, post_type: hancic::models::PostType::Post,
            category_id: None, tags: vec!["标签甲".into()],
        }).await.unwrap();
    }
    let res = app.oneshot(Request::builder().uri("/").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let html = String::from_utf8(axum::body::to_bytes(res.into_body(), 1024*1024).await.unwrap().to_vec()).unwrap();
    assert!(html.contains("heatmap"), "首页应含发布热力图");
    assert!(html.contains("activity-timeline"), "首页应含活动时间轴");
    assert!(html.contains("查看更多文章"), "首页应有查看更多链接");
    assert!(html.contains("标签甲"), "列表项应显示标签");
}

#[tokio::test]
async fn archives_page_lists_all_posts() {
    let (app, pool) = test_app("front-archives").await;
    for t in ["归档文章甲", "归档文章乙"] {
        posts::create_post(&pool, NewPost {
            title: t.into(), content_md: "x".into(), excerpt: None, slug: None,
            status: PostStatus::Published, post_type: hancic::models::PostType::Post,
            category_id: None, tags: vec![],
        }).await.unwrap();
    }
    let res = app.oneshot(Request::builder().uri("/archives").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let html = String::from_utf8(axum::body::to_bytes(res.into_body(), 1024*1024).await.unwrap().to_vec()).unwrap();
    assert!(html.contains("归档文章甲") && html.contains("归档文章乙"));
    assert!(html.contains("全部文章"));
}
