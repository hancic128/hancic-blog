//! T-API 补充：moments 列表/详情/更新、attachments 列表、tags 创建、
//! settings 只读、themes 列表/激活、trails 列表/详情 走查。

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
async fn moments_list_detail_update() {
    let (app, pool): (Router, Db) = test_app("api-extra-moments").await;
    let (_tok, raw) = tokens::generate(&pool, "ci").await.unwrap();
    let token = raw.as_str();

    // 无 token 401
    let (status, _) = send(&app, Method::GET, "/api/moments", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // 先插入两条附件记录供引用
    for (i, kind) in ["image", "file"].iter().enumerate() {
        sqlx::query(
            "INSERT INTO attachments(uuid_name, orig_name, mime, size, kind, path)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(format!("api-extra-{i}"))
        .bind(format!("图{i}.png"))
        .bind("image/png")
        .bind(1024i64)
        .bind(kind)
        .bind(format!("api-extra/{i}.png"))
        .execute(&pool)
        .await
        .unwrap();
    }
    let att1: i64 = sqlx::query_scalar("SELECT id FROM attachments WHERE uuid_name = 'api-extra-0'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let att2: i64 = sqlx::query_scalar("SELECT id FROM attachments WHERE uuid_name = 'api-extra-1'")
        .fetch_one(&pool)
        .await
        .unwrap();

    // 创建
    let (status, body) = send(
        &app,
        Method::POST,
        "/api/moments",
        Some(token),
        Some(json!({"content": "API 测试说说", "attachment_ids": [att1]})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "创建失败: {body}");
    let mid = body["data"]["id"].as_i64().unwrap();

    // 列表命中
    let (status, body) = send(
        &app,
        Method::GET,
        "/api/moments?page=1&page_size=10&q=API",
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["total"], 1);
    assert_eq!(body["data"]["items"][0]["attachments"][0]["id"], att1);

    // 详情
    let (status, body) = send(&app, Method::GET, &format!("/api/moments/{mid}"), Some(token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["content"], "API 测试说说");

    // 更新正文 + 换附件（清空用 []）
    let (status, body) = send(
        &app,
        Method::PATCH,
        &format!("/api/moments/{mid}"),
        Some(token),
        Some(json!({"content": "更新后的说说", "attachment_ids": [att2]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "更新失败: {body}");
    assert_eq!(body["data"]["content"], "更新后的说说");
    assert_eq!(body["data"]["attachments"][0]["id"], att2);

    let (status, body) = send(
        &app,
        Method::PATCH,
        &format!("/api/moments/{mid}"),
        Some(token),
        Some(json!({"attachment_ids": []})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["attachments"].as_array().unwrap().len(), 0);

    // 非法：content 非字符串 / 附件不存在
    let (status, _) = send(
        &app,
        Method::PATCH,
        &format!("/api/moments/{mid}"),
        Some(token),
        Some(json!({"content": 123})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = send(
        &app,
        Method::PATCH,
        &format!("/api/moments/{mid}"),
        Some(token),
        Some(json!({"attachment_ids": [999999]})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = send(&app, Method::GET, "/api/moments/999999", Some(token), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn attachments_list_and_tags_create() {
    let (app, pool): (Router, Db) = test_app("api-extra-att-tags").await;
    let (_tok, raw) = tokens::generate(&pool, "ci").await.unwrap();
    let token = raw.as_str();

    for i in 0..3 {
        sqlx::query(
            "INSERT INTO attachments(uuid_name, orig_name, mime, size, kind, path)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(format!("list-{i}"))
        .bind(if i == 2 { "录像.mp4".to_string() } else { format!("图{i}.png") })
        .bind(if i == 2 { "video/mp4" } else { "image/png" })
        .bind(2048i64)
        .bind(if i == 2 { "video" } else { "image" })
        .bind(format!("list/{i}.bin"))
        .execute(&pool)
        .await
        .unwrap();
    }

    // 附件列表：全部 + kind 过滤 + 关键词
    let (status, body) = send(&app, Method::GET, "/api/attachments?page_size=10", Some(token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["total"], 3);
    let (status, body) = send(
        &app,
        Method::GET,
        "/api/attachments?kind=image",
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["total"], 2, "image 过滤应 2 条: {body}");
    let (status, body) = send(
        &app,
        Method::GET,
        "/api/attachments?q=录像",
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["total"], 1);
    // 非法 kind
    let (status, _) = send(
        &app,
        Method::GET,
        "/api/attachments?kind=audio",
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // 标签创建（幂等同名）+ 超长 400
    let (status, body) = send(
        &app,
        Method::POST,
        "/api/tags",
        Some(token),
        Some(json!({"name": "接口测试"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "创建标签失败: {body}");
    let id1 = body["data"]["id"].as_i64().unwrap();
    let (status, body) = send(
        &app,
        Method::POST,
        "/api/tags",
        Some(token),
        Some(json!({"name": "接口测试"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["id"].as_i64(), Some(id1), "同名应幂等返回同一条");
    let (status, _) = send(
        &app,
        Method::POST,
        "/api/tags",
        Some(token),
        Some(json!({"name": "这个名字太长了"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn settings_themes_trails_read() {
    let (app, pool): (Router, Db) = test_app("api-extra-misc").await;
    let (_tok, raw) = tokens::generate(&pool, "ci").await.unwrap();
    let token = raw.as_str();

    // settings 只读
    let (status, body) = send(&app, Method::GET, "/api/settings", Some(token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body["data"].as_object().map(|o| !o.is_empty()).unwrap_or(false),
        "settings 应返回键值: {body}"
    );
    let (status, _) = send(&app, Method::GET, "/api/settings", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // themes 列表（test_app 已复制仓库 themes/）与激活
    let (status, body) = send(&app, Method::GET, "/api/themes", Some(token), None).await;
    assert_eq!(status, StatusCode::OK);
    let items = body["data"]["items"].as_array().expect("themes items");
    assert!(!items.is_empty(), "应发现内置主题: {body}");
    assert!(
        items.iter().any(|t| t["name"] == "default"),
        "应包含 default 主题"
    );
    let (status, body) = send(
        &app,
        Method::POST,
        "/api/themes/default/activate",
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "激活失败: {body}");
    let (status, _) = send(
        &app,
        Method::POST,
        "/api/themes/no-such/activate",
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // trails：空库列表 200 + 详情 404
    let (status, body) = send(&app, Method::GET, "/api/trails", Some(token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["total"], 0);
    let (status, _) = send(&app, Method::GET, "/api/trails/1", Some(token), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
