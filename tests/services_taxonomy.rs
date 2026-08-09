mod common;
use common::test_config;
use hancic::db;
use hancic::services::taxonomy;

#[tokio::test]
async fn category_crud_and_unique_slug() {
    let cfg = test_config("taxonomy");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    let c = taxonomy::create_category(&pool, "技术", "tech", 1).await.unwrap();
    assert_eq!(c.name, "技术");
    let dup = taxonomy::create_category(&pool, "技术二", "tech", 2).await;
    assert!(dup.is_err());
    let updated = taxonomy::update_category(&pool, c.id, "编程", "code", 0).await.unwrap();
    assert_eq!(updated.slug, "code");
    taxonomy::delete_category(&pool, c.id).await.unwrap();
    assert!(taxonomy::get_category_by_slug(&pool, "code").await.unwrap().is_none());
}

#[tokio::test]
async fn ensure_tag_dedup() {
    let cfg = test_config("tag-dedup");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    let t1 = taxonomy::ensure_tag(&pool, "Rust").await.unwrap();
    let t2 = taxonomy::ensure_tag(&pool, "Rust").await.unwrap();
    assert_eq!(t1.id, t2.id);
    let t3 = taxonomy::ensure_tag(&pool, "rust").await.unwrap();
    assert_eq!(t1.id, t3.id);
    assert_eq!(t3.slug, "rust");
}
