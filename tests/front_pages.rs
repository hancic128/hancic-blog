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

/// 按 id 查文章 UUID（用于 UUID 路由断言）。
async fn post_id_uuid(pool: &db::Db, id: i64) -> String {
    posts::get_post(pool, id).await.unwrap().unwrap().uuid
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
    assert!(html.contains("我的博客"));
}

#[tokio::test]
async fn post_page_renders_markdown() {
    let (app, pool) = test_app("front-post").await;
    let id = create_published_post(&pool, "第一篇文章", None, vec!["rust".into()]).await;
    let post = posts::get_post(&pool, id).await.unwrap().unwrap();

    // 旧 slug 链接现在应永久重定向到 UUID 链接
    let (status, _html) = get_html(&app, "/post/第一篇文章").await;
    assert_eq!(status, StatusCode::PERMANENT_REDIRECT);

    let (status, html) = get_html(&app, &format!("/post/{}", post.uuid)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<h1"));
    assert!(html.contains("正文内容"));
}

#[tokio::test]
async fn post_page_renders_share_meta_tags() {
    let (app, pool) = test_app("front-post-share-meta").await;
    let id = create_published_post(&pool, "分享文章", None, vec![]).await;
    let post = posts::get_post(&pool, id).await.unwrap().unwrap();

    let (status, html) = get_html(&app, &format!("/post/{}", post.uuid)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"<link rel="canonical" href="https://example.test/post/"#));
    assert!(html.contains(r#"property="og:type" content="article""#));
    assert!(html.contains(r#"property="og:title" content="分享文章""#));
    assert!(html.contains(r#"name="twitter:card" content="summary""#));
}

#[tokio::test]
async fn post_page_uses_site_logo_as_absolute_share_image() {
    let (app, pool) = test_app("front-share-logo").await;
    sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES ('site_logo', '/uploads/site/logo.png')")
        .execute(&pool)
        .await
        .unwrap();
    let id = create_published_post(&pool, "Logo 分享文章", None, vec![]).await;
    let post = posts::get_post(&pool, id).await.unwrap().unwrap();

    let (status, html) = get_html(&app, &format!("/post/{}", post.uuid)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"property="og:image" content="https://example.test/uploads/site/logo.png""#));
    assert!(html.contains(r#"name="twitter:image" content="https://example.test/uploads/site/logo.png""#));
    assert!(html.contains(r#"name="twitter:card" content="summary_large_image""#));
}

#[tokio::test]
async fn post_page_omits_share_image_when_logo_missing() {
    let (app, pool) = test_app("front-share-no-logo").await;
    let id = create_published_post(&pool, "无 Logo 分享文章", None, vec![]).await;
    let post = posts::get_post(&pool, id).await.unwrap().unwrap();

    let (status, html) = get_html(&app, &format!("/post/{}", post.uuid)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!html.contains(r#"property="og:image""#));
    assert!(!html.contains(r#"name="twitter:image""#));
}

#[tokio::test]
async fn about_page_renders_share_meta_tags() {
    let (app, pool) = test_app("front-about-share-meta").await;
    posts::create_post(
        &pool,
        NewPost {
            title: "关于本站".into(),
            content_md: "站点正文。".into(),
            excerpt: Some("关于页摘要".into()),
            slug: Some("about".into()),
            status: PostStatus::Published,
            post_type: PostType::Page,
            category_id: None,
            column_id: None,
            tags: vec![],
        },
    ).await.unwrap();

    let (status, html) = get_html(&app, "/about").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"property="og:title" content="关于本站""#));
    assert!(html.contains(r#"property="og:url" content="https://example.test/about""#));
}

#[tokio::test]
async fn page_route_renders_canonical_for_standalone_page() {
    let (app, pool) = test_app("front-share-page-route").await;
    posts::create_post(
        &pool,
        NewPost {
            title: "独立分享页".into(),
            content_md: "页面正文".into(),
            excerpt: Some("页面摘要".into()),
            slug: Some("standalone-share".into()),
            status: PostStatus::Published,
            post_type: PostType::Page,
            category_id: None,
            column_id: None,
            tags: vec![],
        },
    ).await.unwrap();

    let (status, html) = get_html(&app, "/page/standalone-share").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"<link rel="canonical" href="https://example.test/page/standalone-share">"#));
}

#[test]
fn share_context_builds_absolute_urls_from_config() {
    let share = hancic::web::front::share_context(
        "分享配置文章",
        "",
        "正文",
        "/post/分享配置文章",
        "我的博客",
        "https://example.test",
        Some("/uploads/logo.png"),
    );

    assert_eq!(
        share.get("canonical_url").and_then(|v| v.as_str()),
        Some("https://example.test/post/分享配置文章")
    );
    assert_eq!(
        share.get("og_url").and_then(|v| v.as_str()),
        Some("https://example.test/post/分享配置文章")
    );
    assert_eq!(
        share.get("og_image").and_then(|v| v.as_str()),
        Some("https://example.test/uploads/logo.png")
    );
}

#[test]
fn post_page_prefers_excerpt_for_share_description() {
    let share = hancic::web::front::share_context(
        "摘要优先文章",
        "这是手写摘要",
        "# 标题\n\n正文不会被选中",
        "/post/摘要优先文章",
        "我的博客",
        "https://example.test",
        None,
    );

    assert_eq!(share.get("description").and_then(|v| v.as_str()), Some("这是手写摘要"));
}

#[test]
fn page_share_description_falls_back_to_body_text() {
    let share = hancic::web::front::share_context(
        "关于分享",
        "",
        "## 介绍\n\n这里是 **正文摘要来源**，应该去掉 markdown。",
        "/page/share-about",
        "我的博客",
        "https://example.test",
        None,
    );

    let description = share.get("description").and_then(|v| v.as_str()).unwrap();
    assert!(description.contains("正文摘要来源"));
    assert!(!description.contains("**正文摘要来源**"));
}

#[test]
fn share_summary_decodes_entities_and_strips_tags() {
    let share = hancic::web::front::share_context(
        "实体文章",
        "",
        "摘要 <strong>重点</strong>：Rust &amp; Go、&lt;code&gt;、&quot;引号&quot;、&apos;撇号&apos;、&nbsp;空格。",
        "/post/entities",
        "我的博客",
        "https://example.test",
        None,
    );

    let description = share.get("description").and_then(|v| v.as_str()).unwrap();
    assert!(description.contains("重点"));
    assert!(!description.contains("<strong>"), "应剥离 HTML 标签");
    assert!(description.contains("Rust & Go"), "&amp; 应解码为 &");
    assert!(description.contains("<code>"), "&lt;code&gt; 应解码为字面文本");
    assert!(description.contains("引号"), "&quot; 应解码为引号");
    assert!(description.contains("撇号"), "&apos; 应解码为撇号");
    for residue in ["&amp;", "&lt;", "&gt;", "&quot;", "&apos;", "&nbsp;"] {
        assert!(!description.contains(residue), "摘要不应残留实体文本 {residue}");
    }

    // 裸 & 与未知实体应原样保留
    let bare = hancic::web::front::share_context(
        "裸与符号",
        "",
        "Rust & Go、&unknown; 结尾",
        "/post/bare-amp",
        "我的博客",
        "https://example.test",
        None,
    );
    let bare_desc = bare.get("description").and_then(|v| v.as_str()).unwrap();
    assert!(bare_desc.contains("Rust & Go"), "裸 & 不应被吞掉");
    assert!(bare_desc.contains("&unknown;"), "未知实体应原样保留");
}

#[test]
fn share_context_omits_image_when_site_url_or_logo_missing() {
    let no_site = hancic::web::front::share_context(
        "无站点地址",
        "",
        "正文",
        "/post/no-site",
        "我的博客",
        "",
        Some("/uploads/logo.png"),
    );
    assert!(no_site.get("og_image").is_some_and(|v| v.is_null()));

    let no_logo = hancic::web::front::share_context(
        "无 logo",
        "",
        "正文",
        "/post/no-logo",
        "我的博客",
        "https://example.test",
        None,
    );
    assert!(no_logo.get("og_image").is_some_and(|v| v.is_null()));
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
    assert!(
        html.contains("class=\"post-like-count\""),
        "首页文章卡片应渲染点赞容器"
    );
    assert!(html.contains("like-icon"), "首页文章卡片应显示点赞图标");
    assert!(html.contains(">7</span></span>"), "点赞数应绑定在文章卡片点赞容器中");
}

#[tokio::test]
async fn archives_article_card_shows_like_count() {
    let (app, pool) = test_app("front-like-card-archives").await;
    let post_id = create_published_post(&pool, "点赞卡片文章", None, vec![]).await;
    sqlx::query("UPDATE posts SET like_count = 7 WHERE id = ?")
        .bind(post_id)
        .execute(&pool)
        .await
        .unwrap();

    let (status, html) = get_html(&app, "/archives").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        html.contains("class=\"post-like-count\""),
        "归档页文章卡片应渲染点赞容器"
    );
    assert!(html.contains(">7</span></span>"), "点赞数应绑定在文章卡片点赞容器中");
    assert!(html.contains("like-icon"));
}

#[tokio::test]
async fn homepage_article_list_hides_like_sort() {
    let (app, pool) = test_app("front-like-sort").await;
    let first = create_published_post(&pool, "低赞文章", None, vec![]).await;
    let second = create_published_post(&pool, "高赞文章", None, vec![]).await;
    sqlx::query("UPDATE posts SET like_count = 1 WHERE id = ?")
        .bind(first)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE posts SET like_count = 9 WHERE id = ?")
        .bind(second)
        .execute(&pool)
        .await
        .unwrap();

    let (home_status, home_html) = get_html(&app, "/").await;
    assert_eq!(home_status, StatusCode::OK);
    assert!(
        !home_html.contains("href=\"?sort=like_count\""),
        "首页不应提供按点赞排序链接"
    );
    assert!(!home_html.contains("按点赞"), "首页排序入口不应展示按点赞文案");
    let home_high = home_html.find("高赞文章").unwrap();
    let home_low = home_html.find("低赞文章").unwrap();
    assert!(home_high < home_low, "首页高赞文章应排在前面（默认顺序）");

    let (status, html) = get_html(&app, "/archives?sort=like_count").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("href=\"/archives?sort=like_count\""), "归档页应提供按点赞排序链接");
    assert!(html.contains("data-sort=\"like_count\""), "前台排序条应暴露点赞数排序选项");
    let high = html.find("高赞文章").unwrap();
    let low = html.find("低赞文章").unwrap();
    assert!(high < low, "按点赞数排序时高赞文章应排在前面");
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

    let (status, html) = get_html(&app, &format!("/post/{}", post_id_uuid(&pool, post_id).await)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("like-toggle"));
    assert!(html.contains("aria-label=\"点赞这篇文章\""), "文章点赞按钮应带可访问名称");
    assert!(html.contains("<span class=\"like-count\">5</span>"));
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
    assert!(html.contains("aria-label=\"点赞这条说说\""), "说说点赞按钮应带可访问名称");
    assert!(html.contains("<span class=\"like-count\">2</span>"));
}

#[tokio::test]
async fn front_pages_include_like_toggle_script() {
    let (app, pool) = test_app("front-like-script").await;
    let post_id = create_published_post(&pool, "脚本文章", None, vec![]).await;
    hancic::services::moments::create_moment(&pool, "脚本说说", &[])
        .await
        .unwrap();

    let (post_status, post_html) = get_html(&app, &format!("/post/{}", post_id_uuid(&pool, post_id).await)).await;
    assert_eq!(post_status, StatusCode::OK);
    assert!(post_html.contains("/api/likes/toggle"));
    assert!(post_html.contains("[data-like-toggle]"));
    assert!(post_html.contains("content_type: btn.dataset.contentType"));
    assert!(post_html.contains("aria-pressed"));
    assert!(post_html.contains(".like-count"));

    let (moment_status, moment_html) = get_html(&app, "/moments").await;
    assert_eq!(moment_status, StatusCode::OK);
    assert!(moment_html.contains("/api/likes/toggle"));
    assert!(moment_html.contains("[data-like-toggle]"));
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
    // 页面类型不再显示标题（page.html 已移除 h1），正文正常渲染；
    // 标题只允许出现在分享元数据（og:title / twitter:title）里
    assert!(!html.contains("<h1>关于本站"), "页面标题不应以 h1 显示");
    assert!(html.contains("og:title"));
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
