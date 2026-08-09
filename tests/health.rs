use hancic::config::Config;
use tower::ServiceExt;

#[tokio::test]
async fn health_returns_ok() {
    let cfg = Config::default_for_temp_dir();
    let app = hancic::app(cfg).await.unwrap();
    let res = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/health")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);
}
