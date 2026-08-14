mod common;
use common::{extract_csrf, login_admin, start_server_with_cfg, test_config};
use hancic::db;
use hancic::services::{backup, moments, posts, tokens};
use hancic::models::PostStatus;

/// 全量备份 → 破坏数据 → 恢复：导出 zip 应包含数据一致性快照，
/// restore 后重新连库能读到备份前的数据。
#[tokio::test]
async fn export_then_restore_roundtrip() {
    let cfg = test_config("backup");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    posts::create_post(&pool, posts::NewPost {
        title: "备份文章".into(), content_md: "内容".into(), excerpt: None, slug: None,
        status: PostStatus::Published, post_type: hancic::models::PostType::Post,
        category_id: None, column_id: None, tags: vec!["backup".into()],
    }).await.unwrap();
    moments::create_moment(&pool, "备份说说", &[]).await.unwrap();

    let zip_path = cfg.data_dir.join("backup.zip");
    backup::export_all(&cfg.data_dir, &zip_path).await.unwrap();

    // 破坏数据
    posts::delete_post(&pool, 1).await.unwrap();

    backup::restore(&cfg.data_dir, &zip_path).await.unwrap();
    let pool2 = db::init(&cfg.data_dir).await.unwrap();
    assert!(posts::get_post(&pool2, 1).await.unwrap().is_some());
    assert_eq!(moments::list_moments(&pool2, None, false, None, 1, 10).await.unwrap().1, 1);
}

/// 备份页 + 导出下载（CSRF）+ API 备份（Bearer）鉴权链路。
#[tokio::test]
async fn admin_page_export_and_api_backup() {
    let cfg = test_config("backup-http");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    hancic::auth::set_password(&pool, common::TEST_PASSWORD)
        .await
        .unwrap();
    let (addr, client) = start_server_with_cfg(cfg).await;
    let base = format!("http://{addr}");
    assert!(login_admin(&client, &addr).await);

    // 备份页：导出表单 + 恢复上传表单
    let res = client
        .get(format!("{base}/admin/backup"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "备份页应可访问");
    let html = res.text().await.unwrap();
    assert!(
        html.contains("/admin/backup/export") && html.contains("/admin/backup/restore"),
        "页面应含导出与恢复表单"
    );
    let csrf = extract_csrf(&html);

    // 导出下载：application/zip + attachment + 合法 zip（含 hancic.db / meta.json）
    let res = client
        .post(format!("{base}/admin/backup/export"))
        .form(&[("csrf", csrf.as_str())])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "导出应直接返回 zip");
    assert!(
        res.headers()
            .get(reqwest::header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("application/zip")
    );
    let cd = res
        .headers()
        .get(reqwest::header::CONTENT_DISPOSITION)
        .expect("应有 Content-Disposition")
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        cd.contains("attachment") && cd.contains("hancic-backup-"),
        "应提示附件下载: {cd}"
    );
    let bytes = res.bytes().await.unwrap();
    assert_eq!(&bytes[..2], b"PK", "响应体应为 zip");
    let archive = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    let names: Vec<String> = archive.file_names().map(String::from).collect();
    assert!(
        names.iter().any(|n| n == "hancic.db") && names.iter().any(|n| n == "meta.json"),
        "zip 应含 db 与 meta: {names:?}"
    );

    // API 备份：Bearer token 200 + zip；未鉴权 401
    let (_token, plain) = tokens::generate(&pool, "backup-test").await.unwrap();
    let res = client
        .get(format!("{base}/api/backup"))
        .header(reqwest::header::AUTHORIZATION, format!("Bearer {plain}"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "Bearer 应可拉取备份");
    assert!(
        res.headers()
            .get(reqwest::header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("application/zip")
    );
    // 未鉴权（无 session cookie / 无 Bearer）应 401
    let anon = reqwest::Client::new();
    let res = anon
        .get(format!("{base}/api/backup"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 401, "未鉴权应 401");
}

/// 无效备份包（合法 zip 但缺 hancic.db/meta.json）：302 带错误提示，数据不受影响。
#[tokio::test]
async fn restore_rejects_invalid_zip() {
    let cfg = test_config("backup-invalid");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    hancic::auth::set_password(&pool, common::TEST_PASSWORD)
        .await
        .unwrap();
    posts::create_post(&pool, posts::NewPost {
        title: "保留文章".into(), content_md: "x".into(), excerpt: None, slug: None,
        status: PostStatus::Published, post_type: hancic::models::PostType::Post,
        category_id: None, column_id: None, tags: vec![],
    }).await.unwrap();
    let (addr, client) = start_server_with_cfg(cfg).await;
    let base = format!("http://{addr}");
    assert!(login_admin(&client, &addr).await);

    let res = client
        .get(format!("{base}/admin/backup"))
        .send()
        .await
        .unwrap();
    let csrf = extract_csrf(&res.text().await.unwrap());

    // 构造仅含随机文件的合法 zip（缺少 hancic.db / meta.json）
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut buf);
        writer
            .start_file("random.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut writer, b"hello").unwrap();
        writer.finish().unwrap();
    }
    let form = reqwest::multipart::Form::new()
        .text("csrf", csrf)
        .part(
            "backup",
            reqwest::multipart::Part::bytes(buf.into_inner())
                .file_name("bad.zip")
                .mime_str("application/zip")
                .unwrap(),
        );
    let res = client
        .post(format!("{base}/admin/backup/restore"))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302, "无效备份包应重定向回备份页");
    let location = res
        .headers()
        .get(reqwest::header::LOCATION)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(location.contains("msg="), "应带错误提示: {location}");

    // 数据未被破坏
    assert!(posts::get_post(&pool, 1).await.unwrap().is_some(), "恢复失败不应影响现有数据");
}

/// I5：恢复包内 hancic.db 内容损坏（合法 zip 骨架）→ 恢复后 `PRAGMA
/// integrity_check` 失败 → restore 返回错误，提示用 .bak 回滚。
#[tokio::test]
async fn restore_rejects_corrupt_db() {
    let cfg = test_config("backup-corrupt");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    posts::create_post(&pool, posts::NewPost {
        title: "保留文章".into(), content_md: "x".into(), excerpt: None, slug: None,
        status: PostStatus::Published, post_type: hancic::models::PostType::Post,
        category_id: None, column_id: None, tags: vec![],
    }).await.unwrap();

    // 合法 zip 骨架 + 内容非法的 hancic.db
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut buf);
        writer
            .start_file("hancic.db", zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut writer, b"not-a-sqlite-db").unwrap();
        writer
            .start_file("meta.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut writer, br#"{"version":1}"#).unwrap();
        writer.finish().unwrap();
    }
    let zip_path = cfg.data_dir.join("corrupt.zip");
    std::fs::write(&zip_path, buf.into_inner()).unwrap();

    let res = backup::restore(&cfg.data_dir, &zip_path).await;
    assert!(res.is_err(), "损坏 db 的恢复应失败: {res:?}");
    let msg = format!("{:?}", res.unwrap_err());
    assert!(
        msg.contains("完整性校验失败") || msg.contains("回滚"),
        "错误消息应提示完整性校验失败/回滚: {msg}"
    );
}

/// 回归（T22 审查 Critical）：zip-slip 反斜杠绕过。
///
/// 恶意条目名 `uploads\..\..\evil.txt` 在 macOS/Linux 上词法无 `..` 组件，
/// 可过 `enclosed_name()`；归一化（`\`→`/`）后却逃逸 data_dir。restore 必须
/// 整体拒绝，且不产生 data_dir 外文件、不改动现有数据（预检先于改名）。
#[tokio::test]
async fn restore_rejects_backslash_traversal() {
    let cfg = test_config("backup-zipslip");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    posts::create_post(&pool, posts::NewPost {
        title: "保留文章".into(), content_md: "x".into(), excerpt: None, slug: None,
        status: PostStatus::Published, post_type: hancic::models::PostType::Post,
        category_id: None, column_id: None, tags: vec![],
    }).await.unwrap();

    // 恶意 zip：合法骨架（过格式校验）+ 反斜杠逃逸条目（zip 8 写入端原样存名）
    let evil_name = format!(
        "uploads\\..\\..\\evil-{}.txt",
        cfg.data_dir.file_name().unwrap().to_string_lossy()
    );
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut buf);
        writer
            .start_file("hancic.db", zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut writer, b"fake-db").unwrap();
        writer
            .start_file("meta.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut writer, br#"{"version":1}"#).unwrap();
        writer
            .start_file(&evil_name, zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut writer, b"pwned").unwrap();
        writer.finish().unwrap();
    }
    let zip_path = cfg.data_dir.join("malicious.zip");
    std::fs::write(&zip_path, buf.into_inner()).unwrap();
    // 确认写入端原样保留反斜杠条目名（zip 8 不转义），测试才真正命中反斜杠绕过
    let check = zip::ZipArchive::new(std::io::Cursor::new(std::fs::read(&zip_path).unwrap()))
        .unwrap();
    let stored: Vec<String> = check.file_names().map(String::from).collect();
    assert!(
        stored.iter().any(|n| n == &evil_name),
        "恶意条目名应原样入包: {stored:?}"
    );

    let res = backup::restore(&cfg.data_dir, &zip_path).await;
    assert!(res.is_err(), "restore 应拒绝反斜杠逃逸条目: {res:?}");

    // data_dir 外（父级）不应产生逃逸文件
    let parent = cfg.data_dir.parent().unwrap();
    let escaped = parent.join(format!(
        "evil-{}.txt",
        cfg.data_dir.file_name().unwrap().to_string_lossy()
    ));
    assert!(!escaped.exists(), "不应逃逸到 data_dir 外: {}", escaped.display());

    // 现有数据未被改动：data_dir 未被改名（预检在改名前拒绝），原 db 仍可连
    assert!(
        !std::fs::read_dir(parent)
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| {
                let n = e.file_name().to_string_lossy().into_owned();
                n.starts_with(&format!(
                    "{}.bak-",
                    cfg.data_dir.file_name().unwrap().to_string_lossy()
                ))
            }),
        "预检拒绝后不应发生数据目录改名"
    );
    assert!(
        posts::get_post(&pool, 1).await.unwrap().is_some(),
        "拒绝恶意备份不应影响现有数据"
    );
}
