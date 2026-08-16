mod common;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::Router;
use hancic::db::Db;
use hancic::models::{PostStatus, PostType};
use hancic::services::posts::{self, NewPost};
use serde_json::Value;
use tower::ServiceExt;

async fn setup_api_post(tag: &str) -> (Router, i64) {
    let (app, pool): (Router, Db) = common::test_app(tag).await;
    let post = posts::create_post(
        &pool,
        NewPost {
            title: "Like API Post".into(),
            slug: Some("like-api-post".into()),
            content_md: "hello likes".into(),
            excerpt: None,
            category_id: None,
            column_id: None,
            status: PostStatus::Published,
            post_type: PostType::Post,
            tags: vec![],
        },
    )
    .await
    .unwrap();
    (app, post.id)
}

async fn read_json(res: axum::response::Response) -> Value {
    let bytes = to_bytes(res.into_body(), 8 * 1024 * 1024).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn cookie_cookie_header(set_cookie: &str) -> String {
    set_cookie
        .split(';')
        .next()
        .expect("set-cookie should contain name/value")
        .to_string()
}

#[tokio::test]
async fn like_status_sets_visitor_cookie_and_returns_state() {
    let (app, post_id) = setup_api_post("api-likes-status").await;

    let res = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/likes/status?content_type=post&content_id={post_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert!(res.headers().get("set-cookie").is_some());
    let body = read_json(res).await;
    assert_eq!(body["data"]["liked"], false);
    assert_eq!(body["data"]["like_count"], 0);
}

#[tokio::test]
async fn toggle_like_reuses_cookie_and_updates_count() {
    let (app, post_id) = setup_api_post("api-likes-toggle").await;

    let first = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/likes/status?content_type=post&content_id={post_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let cookie = first
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/likes/toggle")
                .header("content-type", "application/json")
                .header("cookie", cookie_cookie_header(&cookie))
                .body(Body::from(format!(
                    r#"{{"content_type":"post","content_id":{post_id}}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = read_json(res).await;
    assert_eq!(body["data"]["liked"], true);
    assert_eq!(body["data"]["like_count"], 1);
}
