mod common;
use hancic::config::Config;
use hancic::db;
use hancic::models::ContentLike;

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

#[tokio::test]
async fn schema_creates_like_tables_and_columns() {
    let cfg = Config::default_for_temp_dir().with_data_dir(common::temp_data_dir("schema-likes"));
    let pool = db::init(&cfg.data_dir).await.unwrap();

    let names: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"
    )
    .fetch_all(&pool)
    .await
    .unwrap()
    .into_iter()
    .map(|s: Option<String>| s.unwrap())
    .collect();

    assert!(names.iter().any(|n| n == "content_likes"));

    let post_cols: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM pragma_table_info('posts') ORDER BY name"
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(post_cols.iter().any(|n| n == "like_count"));

    let moment_cols: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM pragma_table_info('moments') ORDER BY name"
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(moment_cols.iter().any(|n| n == "like_count"));

    let indexes: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='content_likes' ORDER BY name"
    )
    .fetch_all(&pool)
    .await
    .unwrap()
    .into_iter()
    .map(|s: Option<String>| s.unwrap())
    .collect();
    assert!(indexes.iter().any(|n| n == "idx_content_likes_target"));
    assert!(indexes.iter().any(|n| n.starts_with("sqlite_autoindex_content_likes_")));
}

#[tokio::test]
async fn content_likes_rows_decode_into_model_timestamps() {
    let cfg = Config::default_for_temp_dir().with_data_dir(common::temp_data_dir("schema-like-decode"));
    let pool = db::init(&cfg.data_dir).await.unwrap();

    sqlx::query(
        "INSERT INTO content_likes(content_type, content_id, visitor_id, ip_hash, ua_hash) \
         VALUES (?, ?, ?, ?, ?)"
    )
    .bind("post")
    .bind(42_i64)
    .bind("visitor-1")
    .bind("iphash")
    .bind("uahash")
    .execute(&pool)
    .await
    .unwrap();

    let row: ContentLike = sqlx::query_as(
        "SELECT id, content_type, content_id, visitor_id, ip_hash, ua_hash, created_at, updated_at \
         FROM content_likes WHERE visitor_id = ?"
    )
    .bind("visitor-1")
    .fetch_one(&pool)
    .await
    .unwrap();

    let created_at_raw: String = sqlx::query_scalar(
        "SELECT created_at FROM content_likes WHERE visitor_id = ?"
    )
    .bind("visitor-1")
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(row.content_id, 42);
    assert_eq!(row.visitor_id, "visitor-1");
    assert_eq!(row.ip_hash, "iphash");
    assert_eq!(row.ua_hash, "uahash");
    assert!(matches!(row.content_type, hancic::models::LikeContentType::Post));
    assert!(created_at_raw.contains('T'));
    assert!(created_at_raw.ends_with('Z'));
    assert!(row.updated_at >= row.created_at);
}
