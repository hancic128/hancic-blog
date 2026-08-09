mod common;
use hancic::db;
use hancic::config::Config;

#[tokio::test]
async fn schema_creates_all_tables() {
    let cfg = Config::default_for_temp_dir().with_data_dir(common::temp_data_dir("schema"));
    let pool = db::init(&cfg.data_dir).await.unwrap();
    let names: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"
    ).fetch_all(&pool).await.unwrap().into_iter().map(|s: Option<String>| s.unwrap()).collect();
    for t in ["posts","moments","categories","tags","post_tags","moment_attachments",
              "attachments","settings","api_tokens","page_views"] {
        assert!(names.iter().any(|n| n == t), "缺少表 {t}");
    }
    let fts: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type='table' AND name='posts_fts'"
    ).fetch_all(&pool).await.unwrap().into_iter().map(|s: Option<String>| s.unwrap()).collect();
    assert_eq!(fts.len(), 1);
}
