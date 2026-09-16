//! T21：REST API 文章/说说/分类/统计全 CRUD 走查。
//!
//! 覆盖：无 token 401；分类 CRUD（含 slug 冲突 409、404）；文章创建（content_md
//! 原文保留、category_id 关联）、校验 400（空标题/非法 status/分类不存在/缺
//! content_md）、列表分页与 status 筛选、详情、PATCH（含 `category_id: null`
//! 清空）、DELETE 204/404；说说创建删除；统计汇总；health 开放。

mod common;
use common::test_app;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use axum::Router;
use hancic::db::Db;
use hancic::services::tokens;
use serde_json::{Value, json};
use tower::ServiceExt;

/// 发送请求并返回 (状态码, JSON 响应体)；204 等空响应体解析为 Null。
async fn send(
    app: &Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    if let Some(t) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let req = match body {
        Some(v) => builder.body(Body::from(v.to_string())).unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 8 * 1024 * 1024)
        .await
        .unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, value)
}

#[tokio::test]
async fn api_crud_walkthrough() {
    let (app, pool): (Router, Db) = test_app("api-posts").await;
    let (_tok, raw) = tokens::generate(&pool, "ci").await.unwrap();
    let token = raw.as_str();

    // 1. 无 token → 401（统一错误体）
    let (status, body) = send(&app, Method::GET, "/api/posts", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], 401);

    // 2. 建分类（供文章引用）
    let (status, body) = send(
        &app,
        Method::POST,
        "/api/categories",
        Some(token),
        Some(json!({"name":"技术","slug":"tech"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "建分类失败: {body}");
    let cat_id = body["data"]["id"].as_i64().expect("分类应返回 id");
    assert_eq!(body["data"]["name"], "技术");

    // 3. 分类 slug 冲突 → 409
    let (status, body) = send(
        &app,
        Method::POST,
        "/api/categories",
        Some(token),
        Some(json!({"name":"技术杂谈","slug":"tech"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "重复 slug 应 409: {body}");

    // 4. 分类列表
    let (status, body) = send(&app, Method::GET, "/api/categories", Some(token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"][0]["name"], "技术");

    // 5. 新建文章（发布 + 分类 + 标签，content_md 原文必须原样返回）
    let (status, body) = send(
        &app,
        Method::POST,
        "/api/posts",
        Some(token),
        Some(json!({
            "title": "REST API 测试",
            "content_md": "# 你好\n\nmarkdown **原文** 应保留",
            "status": "published",
            "category_id": cat_id,
            "tags": ["api", "测试"],
            "slug": "rest-api-test",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "创建失败: {body}");
    let post_id = body["data"]["id"].as_i64().expect("文章应返回 id");
    assert_eq!(body["data"]["title"], "REST API 测试");
    assert_eq!(body["data"]["content_md"], "# 你好\n\nmarkdown **原文** 应保留");
    assert_eq!(body["data"]["status"], "published");
    assert_eq!(body["data"]["slug"], "rest-api-test");
    assert_eq!(body["data"]["category_id"], json!(cat_id));
    assert!(body["data"]["published_at"].is_string(), "发布文章应有 published_at");
    assert!(body["data"]["views"].is_number());

    // 6. 校验 400：空标题 / 非法 status / 分类不存在 / 缺 content_md
    let (status, _) = send(
        &app,
        Method::POST,
        "/api/posts",
        Some(token),
        Some(json!({"title":"","content_md":"x"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "空标题应 400");
    let (status, body) = send(
        &app,
        Method::POST,
        "/api/posts",
        Some(token),
        Some(json!({"title":"x","content_md":"y","status":"weird"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "非法 status 应 400");
    assert_eq!(body["error"]["code"], 400, "统一错误体: {body}");
    let (status, _) = send(
        &app,
        Method::POST,
        "/api/posts",
        Some(token),
        Some(json!({"title":"x","content_md":"y","category_id":99999})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "不存在的分类应 400");
    let (status, _) = send(
        &app,
        Method::POST,
        "/api/posts",
        Some(token),
        Some(json!({"title":"x"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "缺 content_md 应 400");

    // 7. 列表分页 + 筛选
    let (status, body) = send(
        &app,
        Method::GET,
        "/api/posts?page=1&page_size=5",
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["total"], 1);
    assert_eq!(body["data"]["items"][0]["id"], json!(post_id));
    let (status, body) = send(
        &app,
        Method::GET,
        "/api/posts?status=draft",
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["total"], 0, "draft 筛选不应命中已发布文章");

    // 8. 详情 + 404
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/posts/{post_id}"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["content_md"], "# 你好\n\nmarkdown **原文** 应保留");
    let (status, body) = send(&app, Method::GET, "/api/posts/999999", Some(token), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "不存在应 404: {body}");
    assert_eq!(body["error"]["code"], 404);

    // 9. PATCH：改标题 + `category_id: null` 清空分类
    let (status, body) = send(
        &app,
        Method::PATCH,
        &format!("/api/posts/{post_id}"),
        Some(token),
        Some(json!({"title":"改名","category_id":null})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "PATCH 失败: {body}");
    assert_eq!(body["data"]["title"], "改名");
    assert!(body["data"]["category_id"].is_null(), "null 应清空分类");

    // 10. DELETE 文章 → 204，再删 → 404
    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/posts/{post_id}"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/posts/{post_id}"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 11. 说说：创建 → 201，删除 → 204/404
    let (status, body) = send(
        &app,
        Method::POST,
        "/api/moments",
        Some(token),
        Some(json!({"content":"第一条说说"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "建说说失败: {body}");
    let moment_id = body["data"]["id"].as_i64().expect("说说应返回 id");
    assert_eq!(body["data"]["content"], "第一条说说");
    let (status, body) = send(
        &app,
        Method::POST,
        "/api/moments",
        Some(token),
        Some(json!({"content":""})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "空内容应 400: {body}");
    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/moments/{moment_id}"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/moments/{moment_id}"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 12. 分类 PATCH / DELETE
    let (status, body) = send(
        &app,
        Method::PATCH,
        &format!("/api/categories/{cat_id}"),
        Some(token),
        Some(json!({"name":"工程","sort_order":5})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "改分类失败: {body}");
    assert_eq!(body["data"]["name"], "工程");
    assert_eq!(body["data"]["sort_order"], 5);
    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/categories/{cat_id}"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/categories/{cat_id}"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 13. 统计汇总
    let (status, body) = send(
        &app,
        Method::GET,
        "/api/stats/summary",
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "stats 失败: {body}");
    assert_eq!(body["data"]["total_posts"], 0);
    assert_eq!(body["data"]["total_moments"], 0);
    assert!(body["data"]["trend"].is_array());

    // 14. health 探针保持开放（无需 token）
    let (status, body) = send(&app, Method::GET, "/api/health", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["status"], "ok");
}

/// 拒绝路径（Query 非法、请求体非法 JSON、缺 Content-Type）必须返回统一 JSON
/// 错误体，而不是 axum 默认的 text/plain 400/415。
#[tokio::test]
async fn rejections_return_json_errors() {
    let (app, pool) = test_app("api-reject").await;
    let (_tok, raw) = tokens::generate(&pool, "ci").await.unwrap();
    let auth = format!("Bearer {raw}");

    // Query `page=abc` → 400 JSON 错误体
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/posts?page=abc")
                .header(header::AUTHORIZATION, auth.as_str())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_json_error(res, StatusCode::BAD_REQUEST, "page 必须是整数").await;

    // 请求体非法 JSON → 400 JSON 错误体
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/posts")
                .header(header::AUTHORIZATION, auth.as_str())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("not json"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_json_error(res, StatusCode::BAD_REQUEST, "请求体必须是合法 JSON").await;

    // 缺 Content-Type（合法 JSON body）→ 400 JSON 错误体（统一归 BadRequest）
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/posts")
                .header(header::AUTHORIZATION, auth.as_str())
                .body(Body::from(r#"{"title":"x","content_md":"y"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_json_error(res, StatusCode::BAD_REQUEST, "请求体必须是合法 JSON").await;
}

/// 断言响应为统一 JSON 错误体：状态码 + Content-Type json + error.code/message。
async fn assert_json_error(res: axum::response::Response, status: StatusCode, message: &str) {
    assert_eq!(res.status(), status);
    let ct = res
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.starts_with("application/json"),
        "错误响应 Content-Type 应为 application/json: {ct}"
    );
    let body: Value = serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), 8 * 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(body["error"]["code"], json!(status.as_u16()));
    assert_eq!(body["error"]["message"], message);
}

/// P1-H：PATCH /api/posts/{id} 支持 `published_at` / `updated_at` 字段，
/// RFC3339 字符串解析 → 设值；非法格式 → 400。
#[tokio::test]
async fn patch_post_with_timestamps() {
    let (app, pool): (Router, Db) = test_app("api-posts-ts").await;
    let (_tok, raw) = tokens::generate(&pool, "ci").await.unwrap();
    let token = raw.as_str();

    // 先建一篇文章
    let (_, body) = send(
        &app,
        Method::POST,
        "/api/posts",
        Some(token),
        Some(json!({"title": "回填测试", "content_md": "x", "status": "draft"})),
    )
    .await;
    let id = body["data"]["id"].as_i64().unwrap();
    let custom = "2026-09-15T01:30:00Z";

    // PATCH 同时设 published_at + updated_at
    let (status, body) = send(
        &app,
        Method::PATCH,
        &format!("/api/posts/{id}"),
        Some(token),
        Some(json!({
            "status": "published",
            "published_at": custom,
            "updated_at": custom,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let pub_at = body["data"]["published_at"].as_str().unwrap();
    assert!(pub_at.starts_with("2026-09-15T01:30:00"), "published_at 应以用户时间开头,实际 {pub_at}");
    assert!(pub_at.ends_with('Z'), "published_at 应以 Z 结尾,实际 {pub_at}");
    let upd_at = body["data"]["updated_at"].as_str().unwrap();
    assert!(upd_at.starts_with("2026-09-15T01:30:00"), "updated_at 应跳过自动刷,等于用户值,实际 {upd_at}");
    assert!(upd_at.ends_with('Z'), "updated_at 应以 Z 结尾,实际 {upd_at}");

    // 非法 RFC3339 → 400
    let (status, body) = send(
        &app,
        Method::PATCH,
        &format!("/api/posts/{id}"),
        Some(token),
        Some(json!({"updated_at": "not-a-time"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], json!(400));
}

/// P1-H：POST /api/posts/{id}/timestamps 专用端点。
#[tokio::test]
async fn post_timestamps_endpoint() {
    let (app, pool): (Router, Db) = test_app("api-posts-ts-endpoint").await;
    let (_tok, raw) = tokens::generate(&pool, "ci").await.unwrap();
    let token = raw.as_str();

    let (_, body) = send(
        &app,
        Method::POST,
        "/api/posts",
        Some(token),
        Some(json!({"title": "回填专用端点", "content_md": "不变", "status": "draft"})),
    )
    .await;
    let id = body["data"]["id"].as_i64().unwrap();

    // 1) 设 published_at + updated_at
    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/api/posts/{id}/timestamps"),
        Some(token),
        Some(json!({
            "published_at": "2026-09-14T23:00:00Z",
            "updated_at": "2026-09-15T01:30:00Z",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "设置时间戳失败: {body}");
    let pub_at = body["data"]["published_at"].as_str().unwrap();
    assert!(pub_at.starts_with("2026-09-14T23:00:00"), "published_at 应为用户值,实际 {pub_at}");
    let upd_at = body["data"]["updated_at"].as_str().unwrap();
    assert!(upd_at.starts_with("2026-09-15T01:30:00"), "updated_at 应为用户值,实际 {upd_at}");
    assert_eq!(body["data"]["content_md"], "不变", "正文不应被时间戳端点动");

    // 2) 清空 published_at (null)
    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/api/posts/{id}/timestamps"),
        Some(token),
        Some(json!({"published_at": null})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["data"]["published_at"].is_null(), "null 应清空 published_at");

    // 3) 空 body → 400
    let (status, _body) = send(
        &app,
        Method::POST,
        &format!("/api/posts/{id}/timestamps"),
        Some(token),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // 4) 非法 RFC3339 → 400
    let (status, _body) = send(
        &app,
        Method::POST,
        &format!("/api/posts/{id}/timestamps"),
        Some(token),
        Some(json!({"updated_at": "bad"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // 5) 无 token → 401
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/posts/{id}/timestamps"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"updated_at": "2026-09-15T01:30:00Z"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}
