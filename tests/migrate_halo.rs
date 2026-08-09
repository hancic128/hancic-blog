//! Halo Markdown zip 迁移导入测试：front-matter 解析 + zip 导入（本地图片落库并替换 URL）。

mod common;
use common::test_config;
use hancic::db;
use hancic::services::migrate;
use hancic::services::posts;

/// 生成 fixture zip：posts/hello.md（front-matter + 正文引用外部图与本地图）
/// 外加 assets/local/1.png（真实 PNG bytes）。本地图在 zip 中的路径与正文
/// 引用的相对路径不同层，用于验证图片解析的路径匹配。
fn build_fixture_zip(path: &std::path::Path) {
    use std::io::Write;
    let file = std::fs::File::create(path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();

    let md = "---\ntitle: 你好 halo\ndate: 2024-01-01 10:00:00\ncategories: [技术]\ntags: [rust]\n---\n# 你好 halo\n\n正文外部图 ![外链](https://example.com/x.png) 本地图 ![本地](local/1.png)\n";
    writer.start_file("posts/hello.md", options).unwrap();
    writer.write_all(md.as_bytes()).unwrap();

    writer.start_file("assets/local/1.png", options).unwrap();
    writer.write_all(common::PNG_1x1).unwrap();
    writer.finish().unwrap();
}

#[tokio::test]
async fn parse_front_matter_basic() {
    let raw = "---\ntitle: 你好\ndate: 2024-01-01 10:00:00\ncategories: [技术]\ntags: [rust, 博客]\n---\n# 正文";
    let fm = migrate::parse_front_matter(raw).unwrap();
    assert_eq!(fm.title, "你好");
    assert_eq!(fm.categories, vec!["技术"]);
    assert_eq!(fm.tags, vec!["rust", "博客"]);
}

#[tokio::test]
async fn import_creates_posts_and_local_images() {
    let cfg = test_config("migrate");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    let zip_path = cfg.data_dir.join("halo-export.zip");
    build_fixture_zip(&zip_path);
    // download_images=false：zip 内相对图片也导入（local/1.png 走解包），外部 URL 跳过
    let report = migrate::import_halo_zip(&pool, &cfg.data_dir, &zip_path, false)
        .await
        .unwrap();
    assert_eq!(report.posts_created, 1);
    let p = posts::get_post_by_slug(&pool, "hello").await.unwrap().unwrap();
    assert_eq!(p.title, "你好 halo");
    // 本地图片已入库并替换 URL
    assert!(p.content_md.contains("/uploads/"));
    // 外部图未下载（download_images=false）且不产生失败
    assert_eq!(report.images_downloaded, 0);
    assert_eq!(report.images_failed, 0);
}

/// I7：同 slug 重复导入 → 第二次 posts_skipped=1、posts_created 不变，
/// 不再自动加后缀重建重复文章。
#[tokio::test]
async fn import_same_slug_twice_skips_second() {
    let cfg = test_config("migrate-dup");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    let zip_path = cfg.data_dir.join("halo-dup.zip");
    build_fixture_zip(&zip_path);

    let r1 = migrate::import_halo_zip(&pool, &cfg.data_dir, &zip_path, false)
        .await
        .unwrap();
    assert_eq!(r1.posts_created, 1, "首次导入应创建 1 篇");

    let r2 = migrate::import_halo_zip(&pool, &cfg.data_dir, &zip_path, false)
        .await
        .unwrap();
    assert_eq!(r2.posts_created, 0, "重复导入不应再创建文章");
    assert_eq!(r2.posts_skipped, 1, "重复 slug 应计入跳过");

    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM posts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1, "库中应只有一篇文章");
}

#[tokio::test]
async fn import_downloads_external_images_when_enabled() {
    use axum::http::header;
    use axum::response::IntoResponse;
    use std::io::Write;

    let cfg = test_config("migrate-dl");
    let pool = db::init(&cfg.data_dir).await.unwrap();

    // 本地 mock 图片服务：返回真实 PNG + Content-Type
    let app = axum::Router::new().route(
        "/x.png",
        axum::routing::get(|| async move {
            ([(header::CONTENT_TYPE, "image/png")], common::PNG_1x1).into_response()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service()).await.unwrap();
    });

    // fixture：正文引用带 query/fragment 的外部图（下载成功应计数 + 文件名截断）
    let zip_path = cfg.data_dir.join("halo-dl.zip");
    let url = format!("http://{addr}/x.png?v=2#frag");
    let md = format!("---\ntitle: 下载测试\ndate: 2024-01-02\ntags: [dl]\n---\n# t\n\n![a]({url})\n");
    let file = std::fs::File::create(&zip_path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    writer.start_file("posts/dl.md", options).unwrap();
    writer.write_all(md.as_bytes()).unwrap();
    writer.finish().unwrap();

    let report = migrate::import_halo_zip(&pool, &cfg.data_dir, &zip_path, true)
        .await
        .unwrap();
    assert_eq!(report.posts_created, 1);
    assert_eq!(report.images_downloaded, 1, "外部图下载成功应计数");
    assert_eq!(report.images_failed, 0);
    let p = posts::get_post_by_slug(&pool, "dl").await.unwrap().unwrap();
    assert!(p.content_md.contains("/uploads/"), "正文应替换为本站图");
}
