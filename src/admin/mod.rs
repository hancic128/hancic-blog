//! 后台管理路由：登录 / 登出 / 首启 setup / 仪表盘。
//!
//! T12 统一布局前页面使用内联 HTML，每个页面输出
//! `<meta name="csrf-token" content="...">`（测试依赖它提取 token）。

use crate::AppState;
use crate::auth;
use crate::session;
use axum::Router;
use axum::extract::{ConnectInfo, Form, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use serde::Deserialize;
use std::collections::HashMap;
use std::net::SocketAddr;
use tower_sessions::Session;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/login", get(login_page).post(post_login))
        .route("/logout", get(logout))
        .route("/setup", get(setup_page).post(post_setup))
        .route("/", get(admin_index))
}

/// 302 重定向（FOUND，与测试约定一致）。
fn redirect(location: &str) -> Response {
    (StatusCode::FOUND, [(header::LOCATION, location)]).into_response()
}

#[derive(Deserialize)]
struct LoginForm {
    password: String,
    #[serde(default)]
    csrf: Option<String>,
}

#[derive(Deserialize)]
struct SetupForm {
    password: String,
    #[serde(default)]
    csrf: Option<String>,
}

async fn login_page(
    State(state): State<AppState>,
    session: Session,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let has_password = auth::has_password(&state.db).await.unwrap_or(false);
    let csrf = session::csrf_token(&session).await.unwrap_or_default();
    let setup_tip = if has_password {
        ""
    } else {
        r#"<p class="tip">尚未设置管理员密码，<a href="/admin/setup">前往初始化</a>。</p>"#
    };
    let error_tip = match query.get("error").map(String::as_str) {
        Some("rate") => Some("尝试过于频繁，请 10 分钟后再试。"),
        Some("csrf") => Some("安全校验失败，请刷新页面后重试。"),
        Some(_) => Some("登录失败，请检查密码。"),
        None => None,
    };
    let error_html = error_tip
        .map(|m| format!(r#"<p class="error">{m}</p>"#))
        .unwrap_or_default();
    Html(page(
        "登录 - 寒蝉 Hancic",
        &csrf,
        &format!(
            r#"{setup_tip}
{error_html}
<form method="post" action="/admin/login">
  <input type="hidden" name="csrf" value="{csrf}">
  <label>密码 <input type="password" name="password" required autofocus></label>
  <button type="submit">登录</button>
</form>"#
        ),
    ))
    .into_response()
}

async fn post_login(
    State(state): State<AppState>,
    session: Session,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Form(form): Form<LoginForm>,
) -> Response {
    let ip = addr.ip().to_string();

    // 1. 限流检查（密码校验前）
    if !state.login_limiter.check(&ip, false) {
        return redirect("/admin/login?error=rate");
    }
    // 2. CSRF 校验
    if session::verify_csrf(&session, form.csrf.as_deref())
        .await
        .is_err()
    {
        return redirect("/admin/login?error=csrf");
    }
    // 3. 校验密码并写入会话
    match session::login(&state.db, &session, &form.password).await {
        Ok(()) => {
            state.login_limiter.check(&ip, true); // 成功：清除失败记录
            redirect("/admin")
        }
        Err(_) => {
            state.login_limiter.record_failure(&ip);
            redirect("/admin/login?error=1")
        }
    }
}

async fn logout(session: Session) -> Response {
    let _ = session::logout(&session).await;
    redirect("/admin/login")
}

async fn setup_page(State(state): State<AppState>, session: Session) -> Response {
    if auth::has_password(&state.db).await.unwrap_or(true) {
        return redirect("/admin/login");
    }
    let csrf = session::csrf_token(&session).await.unwrap_or_default();
    Html(page(
        "初始化管理员密码 - 寒蝉 Hancic",
        &csrf,
        &format!(
            r#"<h1>初始化管理员密码</h1>
<p>首次使用请设置管理员密码（至少 {} 个字符）。</p>
<form method="post" action="/admin/setup">
  <input type="hidden" name="csrf" value="{csrf}">
  <label>新密码 <input type="password" name="password" required minlength="{0}"></label>
  <button type="submit">设置并进入后台</button>
</form>"#,
            auth::PASSWORD_MIN_LEN
        ),
    ))
    .into_response()
}

async fn post_setup(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<SetupForm>,
) -> Response {
    if auth::has_password(&state.db).await.unwrap_or(true) {
        return redirect("/admin/login");
    }
    if session::verify_csrf(&session, form.csrf.as_deref())
        .await
        .is_err()
    {
        return redirect("/admin/setup?error=csrf");
    }
    match auth::set_password(&state.db, &form.password).await {
        Ok(()) => {
            // 自动登录
            if session
                .insert(session::USER_ID_KEY, session::ADMIN_ID)
                .await
                .is_err()
            {
                return redirect("/admin/setup?error=1");
            }
            redirect("/admin")
        }
        Err(_) => redirect("/admin/setup?error=1"),
    }
}

async fn admin_index(session: Session) -> Response {
    if session::require_admin(&session).await.is_err() {
        return redirect("/admin/login");
    }
    let csrf = session::csrf_token(&session).await.unwrap_or_default();
    Html(page(
        "仪表盘 - 寒蝉 Hancic",
        &csrf,
        r#"<h1>仪表盘</h1>
<p>后台管理首页，T12 完善。</p>
<a href="/admin/logout">退出登录</a>"#,
    ))
    .into_response()
}

/// 内联页面骨架：输出 csrf-token meta，供表单页与测试使用。
fn page(title: &str, csrf: &str, body: &str) -> String {
    format!(
        "<!DOCTYPE html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <meta name=\"csrf-token\" content=\"{csrf}\"><title>{title}</title></head>\
         <body>{body}</body></html>"
    )
}
