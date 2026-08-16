mod common;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::Router;
use hancic::db::Db;
use hancic::models::{PostStatus, PostType};
use hancic::services::{moments, posts::{self, NewPost}};
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

async fn setup_api_moment(tag: &str) -> (Router, i64) {
    let (app, pool): (Router, Db) = common::test_app(tag).await;
    let moment = moments::create_moment(&pool, "hello moment likes", &[])
        .await
        .unwrap();
    (app, moment.id)
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

fn toggle_like_request(content_type: &str, content_id: i64, cookie: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/api/likes/toggle")
        .header("content-type", "application/json")
        .header("cookie", cookie_cookie_header(cookie))
        .body(Body::from(format!(
            r#"{{"content_type":"{content_type}","content_id":{content_id}}}"#
        )))
        .unwrap()
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
    let cookie = res.headers().get("set-cookie").unwrap().to_str().unwrap();
    assert!(cookie.contains("visitor="));
    assert!(cookie.contains("Path=/"));
    assert!(cookie.contains("HttpOnly"));
    assert!(cookie.contains("SameSite=Lax"));
    assert!(cookie.contains("Max-Age=15552000"));
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
        .oneshot(toggle_like_request("post", post_id, &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = read_json(res).await;
    assert_eq!(body["data"]["liked"], true);
    assert_eq!(body["data"]["like_count"], 1);
}

#[tokio::test]
async fn like_status_and_toggle_support_moment() {
    let (app, moment_id) = setup_api_moment("api-likes-moment").await;

    let first = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/likes/status?content_type=moment&content_id={moment_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let cookie = first
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let body = read_json(first).await;
    assert_eq!(body["data"]["liked"], false);
    assert_eq!(body["data"]["like_count"], 0);

    let toggled = app
        .clone()
        .oneshot(toggle_like_request("moment", moment_id, &cookie))
        .await
        .unwrap();
    assert_eq!(toggled.status(), StatusCode::OK);
    let body = read_json(toggled).await;
    assert_eq!(body["data"]["liked"], true);
    assert_eq!(body["data"]["like_count"], 1);
}

#[tokio::test]
async fn toggle_like_toggles_off_again_for_same_visitor() {
    let (app, post_id) = setup_api_post("api-likes-unlike").await;

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

    let liked = app
        .clone()
        .oneshot(toggle_like_request("post", post_id, &cookie))
        .await
        .unwrap();
    assert_eq!(read_json(liked).await["data"]["liked"], true);

    let unliked = app
        .oneshot(toggle_like_request("post", post_id, &cookie))
        .await
        .unwrap();
    assert_eq!(unliked.status(), StatusCode::OK);
    let body = read_json(unliked).await;
    assert_eq!(body["data"]["liked"], false);
    assert_eq!(body["data"]["like_count"], 0);
}

#[tokio::test]
async fn tampered_visitor_cookie_is_not_trusted_as_is() {
    let (app, post_id) = setup_api_post("api-likes-tampered-cookie").await;

    let issued = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/likes/status?content_type=post&content_id={post_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let issued_cookie = issued
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    let liked = app
        .clone()
        .oneshot(toggle_like_request("post", post_id, &issued_cookie))
        .await
        .unwrap();
    assert_eq!(liked.status(), StatusCode::OK);
    assert_eq!(read_json(liked).await["data"]["liked"], true);

    let tampered_cookie = issued_cookie.replacen("visitor=", "visitor=forged", 1);
    let status = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/likes/status?content_type=post&content_id={post_id}"))
                .header("cookie", cookie_cookie_header(&tampered_cookie))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(status.status(), StatusCode::OK);
    let replacement = status.headers().get("set-cookie").unwrap().to_str().unwrap();
    assert_ne!(cookie_cookie_header(replacement), cookie_cookie_header(&tampered_cookie));
    let body = read_json(status).await;
    assert_eq!(body["data"]["liked"], false);
    assert_eq!(body["data"]["like_count"], 1);
}

#[tokio::test]
async fn excessive_toggle_requests_hit_rate_limit() {
    let (app, post_id) = setup_api_post("api-likes-rate-limit").await;

    let first = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/likes/status?content_type=post&content_id={post_id}"))
                .header("x-real-ip", "198.51.100.24")
                .header("user-agent", "rate-test")
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

    for _ in 0..6 {
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/likes/toggle")
                    .header("content-type", "application/json")
                    .header("cookie", cookie_cookie_header(&cookie))
                    .header("x-real-ip", "198.51.100.24")
                    .header("user-agent", "rate-test")
                    .body(Body::from(format!(
                        r#"{{"content_type":"post","content_id":{post_id}}}"#
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    let limited = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/likes/toggle")
                .header("content-type", "application/json")
                .header("cookie", cookie_cookie_header(&cookie))
                .header("x-real-ip", "198.51.100.24")
                .header("user-agent", "rate-test")
                .body(Body::from(format!(
                    r#"{{"content_type":"post","content_id":{post_id}}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
    let body = read_json(limited).await;
    assert_eq!(body["error"]["code"], 429);
    assert_eq!(body["error"]["message"], "请求过于频繁，请稍后再试");
}
