//! T21：REST API Bearer 鉴权集成测试。
//!
//! 覆盖：未带 token 请求被 401 拒绝（统一错误体 `{error:{code,message}}`）；
//! `tokens::generate` 产出的明文经 `Authorization: Bearer` 放行并创建文章（201）。

mod common;
use common::test_app;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use hancic::services::tokens;
use tower::ServiceExt;

#[tokio::test]
async fn api_requires_token() {
    let (app, _pool) = test_app("api-auth").await;
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/posts")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"title":"x","content_md":"y"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(body["error"]["code"], 401, "应返回统一错误体: {body}");
    assert_eq!(body["error"]["message"], "无效的 API Token");
}

#[tokio::test]
async fn valid_token_creates_post() {
    let (app, pool) = test_app("api-auth2").await;
    let (_tok, raw) = tokens::generate(&pool, "test").await.unwrap();
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/posts")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {raw}"))
                .body(Body::from(
                    r#"{"title":"API 文章","content_md":"来自 agent","status":"published","tags":["ai"]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(body["data"]["title"], "API 文章");
    assert_eq!(body["data"]["content_md"], "来自 agent");
    assert_eq!(body["data"]["status"], "published");
}

#[tokio::test]
async fn wrong_token_rejected() {
    let (app, _pool) = test_app("api-auth3").await;
    let res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/posts")
                .header(header::AUTHORIZATION, "Bearer hc_wrong-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}
