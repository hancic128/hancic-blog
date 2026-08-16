// 供各测试二进制复用的辅助函数；当前两个测试未全部用到，后续任务测试会使用。
#![allow(dead_code)]

use hancic::config::Config;
use hancic::db::Db;
use std::net::SocketAddr;
use std::path::PathBuf;

/// 测试用管理员密码（与 setup 流程配合）。
pub const TEST_PASSWORD: &str = "test-password-123";

/// 1x1 透明 PNG（8bit RGBA），用于上传管线测试。
#[allow(non_upper_case_globals)]
pub const PNG_1x1: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0b, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x60, 0x00, 0x02, 0x00,
    0x00, 0x05, 0x00, 0x01, 0x7a, 0x5e, 0xab, 0x3f, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44,
    0xae, 0x42, 0x60, 0x82,
];

/// ip2region 测试用 xdb 路径：经 `ensure_xdb` 从内嵌资产写出到临时目录
/// （顺带覆盖 `ipregion::ensure_xdb` 的写出逻辑）。
///
/// 每次调用使用唯一目录：测试并行执行时避免共用路径导致互删/竞态。
pub fn xdb_path() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static XDB_SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = XDB_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "hancic-test-xdb-{}-{seq}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    hancic::ipregion::ensure_xdb(&dir).unwrap();
    dir.join("ip2region.xdb")
}

pub fn temp_data_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hancic-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 递归复制目录（主题骨架等静态资源复制到测试数据目录用）。
pub fn copy_recursive(src: &str, dst: &std::path::Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_recursive(&from.to_string_lossy(), &to);
        } else {
            std::fs::copy(&from, &to).unwrap();
        }
    }
}

pub fn test_config(tag: &str) -> Config {
    let mut cfg = Config::default_for_temp_dir().with_data_dir(temp_data_dir(tag));
    cfg.site_url = "https://example.test".into();
    cfg
}

pub async fn test_app(tag: &str) -> (axum::Router, Db) {
    let cfg = test_config(tag);
    // 前台渲染依赖真实主题模板：把仓库 themes/ 复制到测试数据目录，
    // 否则 hancic::app 里 build_tera 找不到模板，前台页面会 500。
    copy_recursive(
        &format!("{}/themes", env!("CARGO_MANIFEST_DIR")),
        &cfg.data_dir.join("themes"),
    );
    let app = hancic::app(cfg.clone()).await.unwrap();
    let pool = hancic::db::init(&cfg.data_dir).await.unwrap();
    (app, pool)
}

/// 用给定配置启动真实 HTTP 服务（带 ConnectInfo 以支持按 IP 限流），
/// 返回地址与 cookie 会话客户端（不跟随重定向）。
///
/// 前台渲染依赖真实主题模板：把仓库 themes/ 复制到数据目录
/// （与 `test_app` 一致），其余初始化由 `hancic::app` 完成。
pub async fn start_server_with_cfg(cfg: Config) -> (SocketAddr, reqwest::Client) {
    copy_recursive(
        &format!("{}/themes", env!("CARGO_MANIFEST_DIR")),
        &cfg.data_dir.join("themes"),
    );
    let app = hancic::app(cfg).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let client = reqwest::Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    (addr, client)
}

/// 启动真实 HTTP 服务（默认配置），返回地址、cookie 会话客户端与数据库。
pub async fn start_server(tag: &str) -> (SocketAddr, reqwest::Client, Db) {
    let cfg = test_config(tag);
    let pool = hancic::db::init(&cfg.data_dir).await.unwrap();
    let (addr, client) = start_server_with_cfg(cfg).await;
    (addr, client, pool)
}

/// 从页面 HTML 提取 `<meta name="csrf-token" content="...">` 的 token。
pub fn extract_csrf(html: &str) -> String {
    let marker = r#"<meta name="csrf-token" content=""#;
    let start = html.find(marker).expect("页面应包含 csrf-token meta") + marker.len();
    let end = html[start..].find('"').expect("csrf-token content 应闭合") + start;
    html[start..end].to_string()
}

/// 通过 /admin/setup 设置管理员密码（成功后自动登录），返回是否 302 到 /admin。
pub async fn setup_password(client: &reqwest::Client, addr: &SocketAddr) -> bool {
    let base = format!("http://{addr}");
    let html = client
        .get(format!("{base}/admin/setup"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let csrf = extract_csrf(&html);
    let res = client
        .post(format!("{base}/admin/setup"))
        .form(&[("password", TEST_PASSWORD), ("csrf", &csrf)])
        .send()
        .await
        .unwrap();
    res.status() == reqwest::StatusCode::FOUND
}

/// 用 TEST_PASSWORD 登录 /admin/login（带 CSRF），返回是否 302。
pub async fn login_admin(client: &reqwest::Client, addr: &SocketAddr) -> bool {
    let base = format!("http://{addr}");
    let html = client
        .get(format!("{base}/admin/login"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let csrf = extract_csrf(&html);
    let res = client
        .post(format!("{base}/admin/login"))
        .form(&[("password", TEST_PASSWORD), ("csrf", &csrf)])
        .send()
        .await
        .unwrap();
    res.status() == reqwest::StatusCode::FOUND
}
