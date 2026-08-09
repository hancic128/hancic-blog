mod common;
use common::test_config;
use hancic::auth;
use hancic::db;

#[tokio::test]
async fn password_hash_verify_roundtrip() {
    let hash = auth::hash_password("correct-horse-123").unwrap();
    assert!(auth::verify_password("correct-horse-123", &hash));
    assert!(!auth::verify_password("wrong", &hash));
    let hash2 = auth::hash_password("correct-horse-123").unwrap();
    assert_ne!(hash, hash2); // 随机盐
}

#[tokio::test]
async fn set_password_requires_min_length() {
    let cfg = test_config("pw-min");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    let r = auth::set_password(&pool, "short").await;
    assert!(r.is_err());
    assert!(!auth::has_password(&pool).await.unwrap());
    auth::set_password(&pool, "a-strong-password!").await.unwrap();
    assert!(auth::has_password(&pool).await.unwrap());
}
