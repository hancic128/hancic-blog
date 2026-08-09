// 供各测试二进制复用的辅助函数；当前两个测试未全部用到，后续任务测试会使用。
#![allow(dead_code)]

use hancic::config::Config;
use hancic::db::Db;
use std::net::SocketAddr;
use std::path::PathBuf;

/// 测试用管理员密码（与 setup 流程配合）。
pub const TEST_PASSWORD: &str = "test-password-123";

pub fn temp_data_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hancic-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn test_config(tag: &str) -> Config {
    Config::default_for_temp_dir().with_data_dir(temp_data_dir(tag))
}

pub async fn test_app(tag: &str) -> (axum::Router, Db) {
    let cfg = test_config(tag);
    let app = hancic::app(cfg.clone()).await.unwrap();
    let pool = hancic::db::init(&cfg.data_dir).await.unwrap();
    (app, pool)
}

/// 启动真实 HTTP 服务（带 ConnectInfo 以支持按 IP 限流），
/// 返回地址、cookie 会话客户端（不跟随重定向）与数据库。
pub async fn start_server(tag: &str) -> (SocketAddr, reqwest::Client, Db) {
    let (app, pool) = test_app(tag).await;
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
