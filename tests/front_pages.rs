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
            column_id: None,
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
async fn homepage_article_card_shows_like_count() {
    let (app, pool) = test_app("front-like-card").await;
    let post_id = create_published_post(&pool, "点赞卡片文章", None, vec![]).await;
    sqlx::query("UPDATE posts SET like_count = 7 WHERE id = ?")
        .bind(post_id)
        .execute(&pool)
        .await
        .unwrap();

    let (status, html) = get_html(&app, "/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("7"));
    assert!(html.contains("like-icon"));
}

#[tokio::test]
async fn post_page_shows_like_button_and_count() {
    let (app, pool) = test_app("front-like-post").await;
    let post_id = create_published_post(&pool, "点赞详情文章", None, vec![]).await;
    sqlx::query("UPDATE posts SET like_count = 5 WHERE id = ?")
        .bind(post_id)
        .execute(&pool)
        .await
        .unwrap();

    let (status, html) = get_html(&app, "/post/点赞详情文章").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("like-toggle"));
    assert!(html.contains("5"));
    assert!(html.contains("like-icon"));
}

#[tokio::test]
async fn moments_page_shows_like_button_and_count() {
    let (app, pool) = test_app("front-like-moment").await;
    let moment = hancic::services::moments::create_moment(&pool, "可点赞说说", &[])
        .await
        .unwrap();
    sqlx::query("UPDATE moments SET like_count = 2 WHERE id = ?")
        .bind(moment.id)
        .execute(&pool)
        .await
        .unwrap();

    let (status, html) = get_html(&app, "/moments").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("moment-like-toggle"));
    assert!(html.contains("2"));
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
            column_id: None,
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
            column_id: None,
            tags: vec![],
        },
    )
    .await
    .unwrap();

    let (status, html) = get_html(&app, "/about").await;
    assert_eq!(status, StatusCode::OK);
    // 页面类型不再显示标题（page.html 已移除 h1），正文正常渲染
    assert!(!html.contains("关于本站"), "页面标题不应显示");
    assert!(html.contains("站点介绍"));
    assert!(html.contains("<h2"));
}

#[tokio::test]
async fn homepage_has_heatmap_activity_and_more_link() {
    let (app, pool) = test_app("front-home-aggregate").await;
    for i in 1..=6 {
        posts::create_post(&pool, NewPost {
            title: format!("聚合页文章{i}"), content_md: "内容".into(), excerpt: None, slug: None,
            status: PostStatus::Published, post_type: hancic::models::PostType::Post,
            category_id: None, column_id: None, tags: vec!["标签甲".into()],
        }).await.unwrap();
    }
    hancic::services::moments::create_moment(&pool, "首页说说一条", &[]).await.unwrap();
    let res = app.oneshot(Request::builder().uri("/").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let html = String::from_utf8(axum::body::to_bytes(res.into_body(), 1024*1024).await.unwrap().to_vec()).unwrap();
    assert!(html.contains("heatmap"), "首页应含更新日历");
    assert!(html.contains("moment-timeline"), "首页应含最近说说时间线");
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
            category_id: None, column_id: None, tags: vec![],
        }).await.unwrap();
    }
    let res = app.oneshot(Request::builder().uri("/archives").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let html = String::from_utf8(axum::body::to_bytes(res.into_body(), 1024*1024).await.unwrap().to_vec()).unwrap();
    assert!(html.contains("归档文章甲") && html.contains("归档文章乙"));
    assert!(html.contains("全部文章"));
}

#[tokio::test]
async fn category_page_shows_category_tags() {
    let (app, pool) = test_app("front-category-tags").await;
    let cat = taxonomy::create_category(&pool, "户外", "outdoor", 0).await.unwrap();
    let other = taxonomy::create_category(&pool, "技术", "tech", 0).await.unwrap();
    create_published_post(&pool, "武功山徒步", Some(cat.id), vec!["徒步".into(), "露营".into()]).await;
    create_published_post(&pool, "Rust 笔记", Some(other.id), vec!["rust".into()]).await;

    let (status, html) = get_html(&app, "/category/outdoor").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("分类：<span class=\"archive-cat\">户外</span>"), "分类名应 accent 区分");
    assert!(html.contains("class=\"category-tags\""), "分类页应展示分类下标签云");
    assert!(html.contains("href=\"/tag/徒步\""), "应显示该分类下文章的标签");
    assert!(html.contains("href=\"/tag/露营\""));
    assert!(!html.contains("href=\"/tag/rust\""), "其他分类的标签不应出现");
}

#[tokio::test]
async fn tag_page_shows_badge_title() {
    let (app, pool) = test_app("front-tag-badge").await;
    create_published_post(&pool, "带标签文章", None, vec!["户外".into()]).await;

    let (status, html) = get_html(&app, "/tag/户外").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("tag-current"), "标签名应保持徽章样式");
    assert!(html.contains("href=\"/tag/户外\""), "徽章应可点击回本标签页");
}

#[tokio::test]
async fn tag_page_has_month_sidebar_and_filter() {
    let (app, pool) = test_app("front-tag-month").await;
    let a = create_published_post(&pool, "标签三月文章", None, vec!["户外".into()]).await;
    let b = create_published_post(&pool, "标签异月文章", None, vec!["户外".into()]).await;
    sqlx::query("UPDATE posts SET published_at = ? WHERE id = ?")
        .bind("2025-03-15T10:00:00Z")
        .bind(a)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE posts SET published_at = ? WHERE id = ?")
        .bind("2024-11-20T10:00:00Z")
        .bind(b)
        .execute(&pool)
        .await
        .unwrap();
    // 无此标签的文章不应计入该标签页月份
    let c = create_published_post(&pool, "无关标签文章", None, vec!["rust".into()]).await;
    sqlx::query("UPDATE posts SET published_at = ? WHERE id = ?")
        .bind("2023-01-05T10:00:00Z")
        .bind(c)
        .execute(&pool)
        .await
        .unwrap();

    let (status, html) = get_html(&app, "/tag/户外").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("href=\"/tag/户外?month=2025-03\""), "侧栏应列出该标签下文章的月份");
    assert!(html.contains("href=\"/tag/户外?month=2024-11\""));
    assert!(!html.contains("2023-01"), "无此标签的月份不应出现");

    let (status, html) = get_html(&app, "/tag/户外?month=2025-03").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("标签三月文章"));
    assert!(!html.contains("标签异月文章"), "月份过滤后不应出现其他月份文章");
}

#[tokio::test]
async fn moments_page_has_month_sidebar_and_filter() {
    let (app, pool) = test_app("front-moments-month").await;
    hancic::services::moments::create_moment(&pool, "三月说说", &[]).await.unwrap();
    hancic::services::moments::create_moment(&pool, "五月说说", &[]).await.unwrap();
    // 让两条说说落在不同月份（created_at 由应用生成，直接改写）
    sqlx::query("UPDATE moments SET created_at = ? WHERE content = ?")
        .bind("2025-03-10T10:00:00Z")
        .bind("三月说说")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE moments SET created_at = ? WHERE content = ?")
        .bind("2025-05-15T10:00:00Z")
        .bind("五月说说")
        .execute(&pool)
        .await
        .unwrap();

    let (status, html) = get_html(&app, "/moments").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("side-months"), "说说页右侧应有按月份时间线");
    assert!(html.contains("href=\"/moments?month=2025-05\""), "月份链接应指向说说页过滤");
    assert!(html.contains("href=\"/moments?month=2025-03\""));

    let (status, html) = get_html(&app, "/moments?month=2025-03").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("「2025-03」的说说"));
    assert!(html.contains("三月说说"));
    assert!(!html.contains("五月说说"), "月份过滤后不应出现其他月份说说");
}

#[tokio::test]
async fn homepage_moments_default_collapsed() {
    let (app, pool) = test_app("front-moments-fold").await;
    for i in 0..3 {
        hancic::services::moments::create_moment(&pool, &format!("折叠说说{i}"), &[]).await.unwrap();
    }

    let (status, html) = get_html(&app, "/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!html.contains("moment-body expanded"), "说说应全部默认折叠");
    assert!(html.contains("aria-expanded=\"false\""), "折叠按钮应标记未展开");
}

#[tokio::test]
async fn archives_page_has_month_sidebar() {
    let (app, pool) = test_app("front-archives-sidebar").await;
    create_published_post(&pool, "筛选文章甲", None, vec![]).await;
    create_published_post(&pool, "筛选文章乙", None, vec![]).await;

    let (status, html) = get_html(&app, "/archives").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("按月份"), "归档页右侧应显示按月份筛选时间线");
    assert!(html.contains("side-months"), "月份应使用时间线样式");
    assert!(html.contains("href=\"/archives?month="), "月份链接应带 month 参数过滤");
    assert!(!html.contains("month-more-btn"), "不足半年月份数不应出现展开按钮");
}

#[tokio::test]
async fn archives_sidebar_show_more_when_many_months() {
    let (app, pool) = test_app("front-archives-more").await;
    // 7 篇分布在 7 个不同月份 → 月份数 > 6，应出现「显示更多月份」按钮
    for i in 0..7 {
        let id = create_published_post(&pool, &format!("跨月筛选{i}"), None, vec![]).await;
        sqlx::query("UPDATE posts SET published_at = ? WHERE id = ?")
            .bind(format!("2025-{:02}-15T10:00:00Z", i + 1))
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
    }

    let (status, html) = get_html(&app, "/archives").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("month-more-btn"), "超过半年月份数应显示「显示更多月份」按钮");
}

#[tokio::test]
async fn archives_month_filter_lists_only_that_month() {
    let (app, pool) = test_app("front-archives-filter").await;
    let a = create_published_post(&pool, "当月文章", None, vec![]).await;
    let b = create_published_post(&pool, "异月文章", None, vec![]).await;
    sqlx::query("UPDATE posts SET published_at = ? WHERE id = ?")
        .bind("2025-03-15T10:00:00Z")
        .bind(a)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE posts SET published_at = ? WHERE id = ?")
        .bind("2024-11-20T10:00:00Z")
        .bind(b)
        .execute(&pool)
        .await
        .unwrap();

    let (status, html) = get_html(&app, "/archives?month=2025-03").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("「2025-03」的文章"), "应显示当前筛选月份标题");
    assert!(html.contains("当月文章"));
    assert!(!html.contains("异月文章"), "其他月份文章不应出现");
}
