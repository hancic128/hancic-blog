//! FTS5 全文搜索集成测试：命中与高亮、空查询提示、特殊字符转义。

mod common;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::test_app;
use hancic::db;
use hancic::models::{PostStatus, PostType};
use hancic::services::posts::{self, NewPost};
use tower::ServiceExt;

/// 便捷：创建一篇已发布文章（FTS 触发器同步索引到 posts_fts）。
async fn create_published_post(pool: &db::Db, title: &str, content: &str) {
    posts::create_post(
        pool,
        NewPost {
            title: title.into(),
            content_md: content.into(),
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
async fn search_finds_matching_posts() {
    let (app, pool) = test_app("search-hit").await;
    for (title, body) in [
        ("Rust 所有权", "借用检查器如何工作"),
        ("Go 并发", "goroutine 使用"),
        ("投资笔记", "定投策略"),
    ] {
        create_published_post(&pool, title, body).await;
    }

    let (status, html) = get_html(&app, "/search?q=Rust").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Rust 所有权"));
    assert!(!html.contains("Go 并发"));
    assert!(html.contains("<mark>Rust</mark>") || html.contains("<mark>rust</mark>"));
}

#[tokio::test]
async fn search_empty_query_returns_prompt() {
    let (app, pool) = test_app("search-empty").await;
    create_published_post(&pool, "测试文章", "一些正文").await;

    let (status, html) = get_html(&app, "/search?q=").await;
    assert_eq!(status, StatusCode::OK);
    // 空查询不列文章，只显示输入提示
    assert!(!html.contains("测试文章"));
    assert!(html.contains("输入关键词"));
}

#[tokio::test]
async fn search_escapes_special_chars() {
    let (app, pool) = test_app("search-special").await;
    for (title, body) in [
        ("Rust 所有权", "借用检查器如何工作"),
        ("Go 并发", "goroutine 使用"),
        ("投资笔记", "定投策略"),
    ] {
        create_published_post(&pool, title, body).await;
    }

    // q = `" -- 恶意'`：双引号/连字符等 FTS5 语法字符应被字面化——
    // 查询要么为空结果要么语法错误按空处理，不能崩溃也不能返回全部文章。
    let q = r#"" -- 恶意'"#;
    let (status, html) = get_html(&app, &format!("/search?q={}", urlencode(q))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!html.contains("Rust 所有权"));
    assert!(!html.contains("Go 并发"));
    assert!(!html.contains("投资笔记"));
}

#[tokio::test]
async fn search_excludes_drafts() {
    let (app, pool) = test_app("search-drafts").await;
    // 草稿（未发布）：posts_fts 触发器会索引它，但搜索不得公开
    posts::create_post(
        &pool,
        NewPost {
            title: "秘密草稿".into(),
            content_md: "内部资料".into(),
            excerpt: None,
            slug: None,
            status: PostStatus::Draft,
            post_type: PostType::Post,
            category_id: None,
            column_id: None,
            tags: vec![],
        },
    )
    .await
    .unwrap();
    create_published_post(&pool, "公开文章", "对外发布的内容").await;

    // 搜草稿词：无结果（total=0），不泄漏草稿正文；搜索词本身会回显在结果行
    let (status, html) = get_html(&app, &format!("/search?q={}", urlencode("秘密草稿"))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("的结果："));
    assert!(!html.contains("post-list-item"));
    assert!(!html.contains("内部资料"));

    // 搜发布词：正常命中
    let (status, html) = get_html(&app, &format!("/search?q={}", urlencode("公开文章"))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("公开文章"));
}

/// C1（Critical）：正文含 `<script>` 的命中片段必须被转义，高亮 `<mark>` 保留。
/// 正文 HTML 在文章页被 pulldown-cmark 转义，但搜索页片段直出——回归测试
/// 确保 snippet() 原文先转义再还原高亮，杜绝存储型 XSS。
#[tokio::test]
async fn search_escapes_html_in_snippet() {
    let (app, pool) = test_app("search-xss").await;
    create_published_post(
        &pool,
        "XSS 测试",
        "正文包含 <script>alert(1)</script> 的恶意内容",
    )
    .await;

    let (status, html) = get_html(&app, &format!("/search?q={}", urlencode("script"))).await;
    assert_eq!(status, StatusCode::OK);

    let snippet_start = html.find("<p class=\"excerpt\">").unwrap();
    let snippet_end = html[snippet_start..].find("</p>").unwrap() + snippet_start;
    let snippet = &html[snippet_start..snippet_end];

    assert!(
        !snippet.contains("<script>"),
        "snippet 不得出现未转义的 <script>: {snippet}"
    );
    assert!(
        snippet.contains("&lt;<mark>script</mark>&gt;alert(1)&lt;/<mark>script</mark>&gt;"),
        "snippet 应转义 HTML 并保留高亮: {snippet}"
    );
}

#[tokio::test]
async fn search_excludes_pages() {
    let (app, pool) = test_app("search-pages").await;
    // 独立页（page）：索引了但不应出现在文章搜索结果
    posts::create_post(
        &pool,
        NewPost {
            title: "独立页面".into(),
            content_md: "关于页内容".into(),
            excerpt: None,
            slug: Some("standalone-page".into()),
            status: PostStatus::Published,
            post_type: PostType::Page,
            category_id: None,
            column_id: None,
            tags: vec![],
        },
    )
    .await
    .unwrap();

    let (status, html) = get_html(&app, &format!("/search?q={}", urlencode("独立页面"))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("的结果："));
    assert!(!html.contains("post-list-item"));
    assert!(!html.contains("关于页内容"));
}

/// 简单百分号编码（查询串中的非保留字符）。
fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}
