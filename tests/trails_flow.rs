//! 徒步轨迹集成测试：后台上传（合法/非法）→ 前台渲染（总览/详情/404）+ CRUD。

mod common;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{extract_csrf, login_admin, start_server_with_cfg, test_app, test_config};
use hancic::db;
use hancic::services::trails;
use tower::ServiceExt;

/// 合法 GPX：3 个点，含海拔与时间（两步路导出格式的子集）。
const GPX_VALID: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1" creator="两步路">
  <metadata><name>测试轨迹</name></metadata>
  <trk>
    <trkseg>
      <trkpt lat="31.2304" lon="121.4737"><ele>100</ele><time>2026-01-01T00:00:00Z</time></trkpt>
      <trkpt lat="31.2394" lon="121.4737"><ele>150</ele><time>2026-01-01T00:01:00Z</time></trkpt>
      <trkpt lat="31.2394" lon="121.4827"><ele>100</ele><time>2026-01-01T00:02:00Z</time></trkpt>
    </trkseg>
  </trk>
</gpx>"#;

/// 非法 GPX：无轨迹点。
const GPX_NO_TRKPT: &str = r#"<gpx version="1.1"><trk><name>空轨迹</name></trk></gpx>"#;

/// 后台 GPX 上传（multipart：csrf + 可选 name + file）。
async fn upload_gpx(
    client: &reqwest::Client,
    base: &str,
    csrf: &str,
    file_name: &str,
    data: &[u8],
    name: &str,
) -> reqwest::Response {
    let form = reqwest::multipart::Form::new()
        .text("csrf", csrf.to_string())
        .text("name", name.to_string())
        .part(
            "file",
            reqwest::multipart::Part::bytes(data.to_vec())
                .file_name(file_name.to_string())
                .mime_str("application/gpx+xml")
                .unwrap(),
        );
    client
        .post(format!("{base}/admin/trails/upload"))
        .multipart(form)
        .send()
        .await
        .unwrap()
}

/// 后台列表页的 CSRF（cookie 会话内有效）。
async fn admin_csrf(client: &reqwest::Client, base: &str) -> String {
    let html = client
        .get(format!("{base}/admin/trails"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    extract_csrf(&html)
}

async fn get_html(app: &axum::Router, uri: &str) -> (StatusCode, String) {
    let res = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    (status, String::from_utf8(bytes).unwrap())
}

#[tokio::test]
async fn admin_upload_valid_gpx_then_front_display() {
    let cfg = test_config("trails-upload");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    hancic::auth::set_password(&pool, common::TEST_PASSWORD)
        .await
        .unwrap();
    let (addr, client) = start_server_with_cfg(cfg.clone()).await;
    let base = format!("http://{addr}");
    assert!(login_admin(&client, &addr).await);

    // 上传（名称留空 → 用 GPX 内 metadata name）
    let csrf = admin_csrf(&client, &base).await;
    let res = upload_gpx(&client, &base, &csrf, "trail.gpx", GPX_VALID.as_bytes(), "").await;
    assert_eq!(res.status(), reqwest::StatusCode::FOUND, "上传应 302 回列表");

    // 落库校验：统计计算正确 + 文件落盘
    let items = trails::list_trails(&pool, trails::TrailSort::Recent).await.unwrap();
    assert_eq!(items.len(), 1);
    let t = &items[0];
    assert_eq!(t.name, "测试轨迹");
    assert_eq!(t.point_count, 3);
    assert!(t.distance_m.unwrap() > 1800.0 && t.distance_m.unwrap() < 1900.0, "distance={:?}", t.distance_m);
    assert!((t.elevation_gain_m.unwrap() - 50.0).abs() < 1e-6);
    assert!((t.elevation_loss_m.unwrap() - 50.0).abs() < 1e-6);
    assert_eq!(t.moving_seconds, Some(120));
    assert_eq!(t.max_elevation_m, Some(150.0));
    assert_eq!(t.min_elevation_m, Some(100.0));
    assert!(t.simplified.contains("31.2304"), "simplified 含起点: {}", t.simplified);
    // 完整坐标 JSON 落盘（详情页直接加载）
    let coords = trails::load_full_coords(&cfg.data_dir.join("trails"), t.id).unwrap();
    assert_eq!(coords.len(), 3);

    // 后台列表渲染（名称 + 格式化里程）
    let html = client
        .get(format!("{base}/admin/trails"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(html.contains("测试轨迹"), "后台列表含轨迹名");
    assert!(html.contains("1.9 km"), "后台列表含里程: {html}");

    // 前台总览：地图容器 + 轨迹卡片
    let html = client
        .get(format!("{base}/trails"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(html.contains(r#"id="trails-map""#), "总览含地图容器");
    assert!(html.contains("测试轨迹"), "总览含轨迹卡片");

    // 前台详情：地图 + 数据卡片
    let html = client
        .get(format!("{base}/trails/{}", t.id))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(html.contains(r#"id="trail-map""#), "详情含地图容器");
    assert!(html.contains("累计爬升"), "详情含爬升卡片");
    assert!(html.contains("累计下降"), "详情含下降卡片");
    assert!(html.contains("运动时长"), "详情含时长卡片");
    assert!(html.contains("均速"), "详情含均速卡片");
}

#[tokio::test]
async fn admin_upload_rejects_invalid_gpx_and_extension() {
    let cfg = test_config("trails-reject");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    hancic::auth::set_password(&pool, common::TEST_PASSWORD)
        .await
        .unwrap();
    let (addr, client) = start_server_with_cfg(cfg.clone()).await;
    let base = format!("http://{addr}");
    assert!(login_admin(&client, &addr).await);
    let csrf = admin_csrf(&client, &base).await;

    // 无轨迹点：拒绝并回显错误
    let res = upload_gpx(&client, &base, &csrf, "empty.gpx", GPX_NO_TRKPT.as_bytes(), "").await;
    assert_eq!(res.status(), reqwest::StatusCode::FOUND);
    let loc = res.headers().get("location").unwrap().to_str().unwrap().to_string();
    assert!(loc.contains("msg="), "失败应带 msg: {loc}");
    assert!(trails::list_trails(&pool, trails::TrailSort::Recent).await.unwrap().is_empty());

    // 非 .gpx 扩展名：拒绝
    let res = upload_gpx(&client, &base, &csrf, "trail.txt", GPX_VALID.as_bytes(), "").await;
    assert_eq!(res.status(), reqwest::StatusCode::FOUND);
    let loc = res.headers().get("location").unwrap().to_str().unwrap().to_string();
    assert!(loc.contains("msg="), "非 gpx 应拒绝: {loc}");
    assert!(trails::list_trails(&pool, trails::TrailSort::Recent).await.unwrap().is_empty());

    // 缺文件字段：拒绝
    let form = reqwest::multipart::Form::new().text("csrf", csrf.clone());
    let res = client
        .post(format!("{base}/admin/trails/upload"))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::FOUND);
    assert!(trails::list_trails(&pool, trails::TrailSort::Recent).await.unwrap().is_empty());

    // 上传前未带 csrf：拒绝
    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(GPX_VALID.as_bytes().to_vec())
            .file_name("t.gpx")
            .mime_str("application/gpx+xml")
            .unwrap(),
    );
    let res = client
        .post(format!("{base}/admin/trails/upload"))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::FOUND);
    assert!(trails::list_trails(&pool, trails::TrailSort::Recent).await.unwrap().is_empty());
}

#[tokio::test]
async fn trail_crud_update_name_and_delete_files() {
    let cfg = test_config("trails-crud");
    let pool = db::init(&cfg.data_dir).await.unwrap();
    hancic::auth::set_password(&pool, common::TEST_PASSWORD)
        .await
        .unwrap();
    let (addr, client) = start_server_with_cfg(cfg.clone()).await;
    let base = format!("http://{addr}");
    assert!(login_admin(&client, &addr).await);
    let csrf = admin_csrf(&client, &base).await;
    let res = upload_gpx(&client, &base, &csrf, "trail.gpx", GPX_VALID.as_bytes(), "").await;
    assert_eq!(res.status(), reqwest::StatusCode::FOUND);
    let t = &trails::list_trails(&pool, trails::TrailSort::Recent).await.unwrap()[0];
    let id = t.id;
    let gpx_path = cfg.data_dir.join("trails").join(&t.file_path);
    let json_path = cfg.data_dir.join("trails").join(format!("{id}.json"));
    assert!(gpx_path.exists() && json_path.exists());

    // 编辑名称/描述（后台接口）
    let res = client
        .post(format!("{base}/admin/trails/{id}/update"))
        .form(&[("csrf", csrf.as_str()), ("name", "改名轨迹"), ("description", "路线描述")])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::FOUND);
    let t = trails::get_trail(&pool, id).await.unwrap().unwrap();
    assert_eq!(t.name, "改名轨迹");
    assert_eq!(t.description, "路线描述");
    let html = client
        .get(format!("{base}/trails/{id}"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(html.contains("改名轨迹"), "详情页显示新名称");
    assert!(html.contains("路线描述"), "详情页显示描述");

    // 删除：DB 行 + 磁盘文件
    let res = client
        .post(format!("{base}/admin/trails/{id}/delete"))
        .form(&[("csrf", &csrf)])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::FOUND);
    assert!(trails::list_trails(&pool, trails::TrailSort::Recent).await.unwrap().is_empty());
    assert!(!gpx_path.exists(), "GPX 文件应删除");
    assert!(!json_path.exists(), "完整坐标 JSON 应删除");
}

#[tokio::test]
async fn trail_pages_render_with_404() {
    let (app, pool) = test_app("trails-pages").await;
    // 直接经 service 层造一条轨迹（绕过上传接口），验证前台渲染。
    // ⚠️ 不能再次调用 temp_data_dir：那会清空重建数据目录（删除 SQLite 文件），
    // 已打开的连接指向旧 inode 导致 flaky。这里只拼路径（import_gpx 自建目录）。
    let trails_dir = std::env::temp_dir()
        .join(format!("hancic-test-trails-pages-{}", std::process::id()))
        .join("trails");
    let t = trails::import_gpx(&pool, &trails_dir, "", "", "", GPX_VALID.as_bytes())
        .await
        .unwrap();

    let (status, html) = get_html(&app, "/trails").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"id="trails-map""#));
    assert!(html.contains("测试轨迹"));

    let (status, html) = get_html(&app, &format!("/trails/{}", t.id)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"id="trail-map""#));
    assert!(html.contains("累计爬升"));
    assert!(html.contains("起点"));
    assert!(html.contains("终点"));

    // 不存在 → 404（error.html 渲染）
    let (status, _html) = get_html(&app, "/trails/99999").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _html) = get_html(&app, "/trails/abc").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
