//! T：REST API 专栏 CRUD + 专栏文章增减走查。
//!
//! 覆盖：无 token 401；创建（201/slug 自动生成含中文）；校验 400（空名/超长名/
//! 超长描述）；slug 冲突 409；文章加入专栏（column_id 落库）、专栏文章列表、
//! 列表计数；文章不存在 400 / 专栏不存在 404；PATCH 改名改描述（slug 保留）；
//! 移除文章 204 + column_id 置空；删除专栏 204/404。

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
async fn api_columns_walkthrough() {
    let (app, pool): (Router, Db) = test_app("api-columns").await;
    let (_tok, raw) = tokens::generate(&pool, "ci").await.unwrap();
    let token = raw.as_str();

    // 1. 无 token → 401（统一错误体）
    let (status, body) = send(&app, Method::GET, "/api/columns", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], 401);

    // 2. 创建专栏（201，slug 自动生成——中文原样保留，与后台一致）
    let (status, body) = send(
        &app,
        Method::POST,
        "/api/columns",
        Some(token),
        Some(json!({"name":"工程思维","description":"把工程思维用到生活和决策里"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "建专栏失败: {body}");
    let col_id = body["data"]["id"].as_i64().expect("专栏应返回 id");
    assert_eq!(body["data"]["name"], "工程思维");
    assert_eq!(body["data"]["slug"], "工程思维");
    assert_eq!(body["data"]["description"], "把工程思维用到生活和决策里");
    // 创建响应为原始 Column（无 posts 计数，计数仅在 list 端点富化，见步骤 7）

    // 3. 校验 400：空名 / 超长名 / 超长描述
    let (status, _) = send(
        &app,
        Method::POST,
        "/api/columns",
        Some(token),
        Some(json!({"name":""})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "空名称应 400");
    let (status, body) = send(
        &app,
        Method::POST,
        "/api/columns",
        Some(token),
        Some(json!({"name":"一二三四五六七八九"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "超长名称应 400: {body}");
    let (status, _) = send(
        &app,
        Method::POST,
        "/api/columns",
        Some(token),
        Some(json!({"name":"测试","description":"这".repeat(51)})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "超长描述应 400");

    // 4. 显式 slug 冲突 → 409
    let (status, body) = send(
        &app,
        Method::POST,
        "/api/columns",
        Some(token),
        Some(json!({"name":"另一个专栏","slug":"工程思维"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "重复 slug 应 409: {body}");

    // 5. 建一篇已发布文章，加入专栏 → column_id 落库
    let (status, body) = send(
        &app,
        Method::POST,
        "/api/posts",
        Some(token),
        Some(json!({"title":"开篇","content_md":"正文","status":"published"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "建文章失败: {body}");
    let post_id = body["data"]["id"].as_i64().expect("文章应返回 id");
    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/api/columns/{col_id}/posts"),
        Some(token),
        Some(json!({"post_id": post_id})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "加入专栏失败: {body}");
    assert_eq!(body["data"]["column_id"], json!(col_id));

    // 6. 专栏文章列表（分页结构）+ 专栏列表计数
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/columns/{col_id}/posts"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["total"], 1);
    assert_eq!(body["data"]["items"][0]["id"], json!(post_id));
    let (status, body) = send(&app, Method::GET, "/api/columns", Some(token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"][0]["id"], json!(col_id));
    assert_eq!(body["data"][0]["posts"], 1);

    // 7. 失败路径：文章不存在 400 / 专栏不存在 404
    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/api/columns/{col_id}/posts"),
        Some(token),
        Some(json!({"post_id": 999999})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "不存在的文章应 400: {body}");
    let (status, _) = send(
        &app,
        Method::POST,
        "/api/columns/999999/posts",
        Some(token),
        Some(json!({"post_id": post_id})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "不存在的专栏应 404");
    let (status, _) = send(
        &app,
        Method::GET,
        "/api/columns/999999/posts",
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "不存在专栏的文章列表应 404");

    // 8. PATCH：改名/改描述（slug 保留，避免前台链接失效）
    let (status, body) = send(
        &app,
        Method::PATCH,
        &format!("/api/columns/{col_id}"),
        Some(token),
        Some(json!({"name":"工程方法论","description":"新描述"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "改专栏失败: {body}");
    assert_eq!(body["data"]["name"], "工程方法论");
    assert_eq!(body["data"]["description"], "新描述");
    assert_eq!(body["data"]["slug"], "工程思维", "更新不应改 slug");

    // 9. 移除文章 → 204，column_id 置空（文章保留）
    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/columns/{col_id}/posts/{post_id}"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/posts/{post_id}"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["data"]["column_id"].is_null(), "移除后 column_id 应清空");

    // 10. 删除专栏 → 204，再删 → 404
    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/columns/{col_id}"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/columns/{col_id}"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
