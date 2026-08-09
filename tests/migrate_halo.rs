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
