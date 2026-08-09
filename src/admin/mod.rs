//! 后台管理路由：登录 / 登出 / 首启 setup / 仪表盘。
//!
//! T12 起页面统一渲染到 `assets/admin_templates/`（`include_str!` 编译期嵌入，
//! `build_tera` 注册进 `AppState::tera_admin`），`layout.html` 提供侧边栏导航
//! 与 CSRF meta。后台页全部附 `Cache-Control: no-store`（M19），渲染走 tera
//! 自动转义（M22：输出用户内容一律 `{{ }}`，脚本/HTML 不落地在模板里）。

use crate::error::AppError;
use crate::models::{PostStatus, PostType};
use crate::services::{posts as posts_service, settings, stats};
use crate::AppState;
use crate::{auth, session};
use axum::Router;
use axum::extract::{ConnectInfo, Form, OriginalUri, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use chrono::{DateTime, Days, FixedOffset, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::SocketAddr;
use tera::{Context, Tera};
use tower_sessions::Session;

pub mod attachments;
pub mod moments;
pub mod posts;
pub mod taxonomy;

/// 仪表盘最近草稿条数。
const DASHBOARD_DRAFT_LIMIT: i64 = 5;
/// 近 30 日趋势窗口（含今天）。
const TREND_DAYS: i64 = 30;
/// 后台时间显示时区偏移（Asia/Shanghai，UTC+8；T17 允许配置时区后再调整）。
const TZ_OFFSET_SECS: i32 = 8 * 3600;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/login", get(login_page).post(post_login))
        .route("/logout", get(logout))
        .route("/setup", get(setup_page).post(post_setup))
        .route("/", get(admin_index))
        .route("/posts", get(posts::list).post(posts::create))
        .route("/posts/new", get(posts::new_page))
        .route("/posts/{id}/edit", get(posts::edit_page))
        .route("/posts/{id}/update", post(posts::update))
        .route("/posts/{id}/delete", post(posts::delete))
        .route("/posts/{id}/autosave", post(posts::autosave))
        .route("/moments", get(moments::list).post(moments::create))
        .route("/moments/{id}/delete", post(moments::delete))
        .route("/attachments", get(attachments::list))
        .route("/attachments/{id}/delete", post(attachments::delete))
        .route("/attachments/upload", get(attachments::upload_page))
        .route("/taxonomy", get(taxonomy::list))
        .route("/taxonomy/categories", post(taxonomy::create_category))
        .route("/taxonomy/categories/{id}/update", post(taxonomy::update_category))
        .route("/taxonomy/categories/{id}/delete", post(taxonomy::delete_category))
        .route("/taxonomy/tags", post(taxonomy::create_tag))
        .route("/taxonomy/tags/{id}/delete", post(taxonomy::delete_tag))
}

/// 注册后台模板集：`include_str!` 编译期嵌入，全部为仓库内嵌模板，
/// `add_raw_templates` 仅做语法校验，理论上不会失败。
pub fn build_tera() -> Tera {
    let mut tera = Tera::default();
    tera.add_raw_templates(vec![
        ("layout.html", include_str!("../../assets/admin_templates/layout.html")),
        ("login.html", include_str!("../../assets/admin_templates/login.html")),
        ("setup.html", include_str!("../../assets/admin_templates/setup.html")),
        ("dashboard.html", include_str!("../../assets/admin_templates/dashboard.html")),
        ("posts_list.html", include_str!("../../assets/admin_templates/posts_list.html")),
        ("post_edit.html", include_str!("../../assets/admin_templates/post_edit.html")),
        ("moments.html", include_str!("../../assets/admin_templates/moments.html")),
        ("attachments.html", include_str!("../../assets/admin_templates/attachments.html")),
        ("taxonomy.html", include_str!("../../assets/admin_templates/taxonomy.html")),
    ])
    .expect("内嵌后台模板注册失败");
    tera
}

/// 302 重定向（FOUND，与测试约定一致）。
pub(crate) fn redirect(location: &str) -> Response {
    (StatusCode::FOUND, [(header::LOCATION, location)]).into_response()
}

/// 后台页基础上下文：site_name / csrf / admin_nav（layout.html 消费）。
pub(crate) async fn base_ctx(state: &AppState, session: &Session, path: &str) -> (Context, String) {
    let csrf = session::csrf_token(session).await.unwrap_or_default();
    let site_name = settings::get(&state.db, "site_name")
        .await
        .ok()
        .flatten()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| state.config.site_name.clone());
    let mut ctx = Context::new();
    ctx.insert("site_name", &site_name);
    ctx.insert("csrf", &csrf);
    ctx.insert("admin_nav", &nav_value(&admin_nav(path)));
    (ctx, csrf)
}

/// 渲染后台模板并附 `Cache-Control: no-store`：后台内容动态且页面含 CSRF，
/// 禁止浏览器/中间层缓存。
pub(crate) fn render_admin(state: &AppState, template: &str, ctx: &Context) -> Response {
    let html = match state.tera_admin.render(template, ctx) {
        Ok(html) => html,
        Err(e) => {
            tracing::error!("渲染后台模板 {template} 失败: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, Html("后台页面渲染失败"))
                .into_response();
        }
    };
    ([(header::CACHE_CONTROL, "no-store")], Html(html)).into_response()
}

// ---------- 侧边栏导航 ----------

/// 导航项（url 为后台路由或前台链接）。
struct NavItem {
    url: &'static str,
    label: &'static str,
    active: bool,
}

/// 侧边栏 11 模块 + 「查看站点」；active 按当前请求路径匹配。
fn admin_nav(path: &str) -> Vec<NavItem> {
    let items: [(&str, &str); 12] = [
        ("/admin", "仪表盘"),
        ("/admin/posts", "文章"),
        ("/admin/moments", "说说"),
        ("/admin/attachments", "附件库"),
        ("/admin/taxonomy", "分类标签"),
        ("/admin/settings", "站点设置"),
        ("/admin/themes", "主题"),
        ("/admin/stats", "统计"),
        ("/admin/tokens", "API Token"),
        ("/admin/backup", "备份"),
        ("/admin/import", "迁移导入"),
        ("/", "查看站点"),
    ];
    items
        .iter()
        .map(|(url, label)| NavItem {
            url,
            label,
            active: is_active(path, url),
        })
        .collect()
}

/// 路径匹配：仪表盘仅精确命中 `/admin`（含结尾斜杠），其余按前缀匹配，
/// 避免 `/admin/settings` 与 `/admin/setup` 等前导相似路径互相误伤。
fn is_active(path: &str, url: &str) -> bool {
    if url == "/admin" {
        return path == "/admin" || path == "/admin/";
    }
    if url == "/" {
        return false; // 查看站点永不高亮
    }
    path.starts_with(url)
}

fn nav_value(items: &[NavItem]) -> Value {
    json!(items
        .iter()
        .map(|i| json!({ "url": i.url, "label": i.label, "active": i.active }))
        .collect::<Vec<_>>())
}

// ---------- 表单 ----------

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

// ---------- 登录 / 登出 / 初始化 ----------

async fn login_page(
    State(state): State<AppState>,
    session: Session,
    Query(query): Query<HashMap<String, String>>,
    uri: OriginalUri,
) -> Response {
    let has_password = auth::has_password(&state.db).await.unwrap_or(false);
    let (mut ctx, _csrf) = base_ctx(&state, &session, uri.path()).await;
    let error_tip = match query.get("error").map(String::as_str) {
        Some("rate") => Some("尝试过于频繁，请 10 分钟后再试。"),
        Some("csrf") => Some("安全校验失败，请刷新页面后重试。"),
        Some(_) => Some("登录失败，请检查密码。"),
        None => None,
    };
    ctx.insert("has_password", &has_password);
    ctx.insert("error_tip", &error_tip.unwrap_or_default());
    render_admin(&state, "login.html", &ctx)
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

async fn setup_page(State(state): State<AppState>, session: Session, uri: OriginalUri) -> Response {
    if auth::has_password(&state.db).await.unwrap_or(true) {
        return redirect("/admin/login");
    }
    let (mut ctx, _csrf) = base_ctx(&state, &session, uri.path()).await;
    ctx.insert("password_min_len", &auth::PASSWORD_MIN_LEN);
    render_admin(&state, "setup.html", &ctx)
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

// ---------- 仪表盘 ----------

async fn admin_index(
    State(state): State<AppState>,
    session: Session,
    uri: OriginalUri,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return redirect("/admin/login");
    }
    let (mut ctx, _csrf) = base_ctx(&state, &session, uri.path()).await;
    if let Err(e) = fill_dashboard(&state, &mut ctx).await {
        tracing::error!("仪表盘数据查询失败: {e:?}");
    }
    render_admin(&state, "dashboard.html", &ctx)
}

/// 仪表盘上下文：四类总数 + 近 30 日趋势 + 最近草稿。
async fn fill_dashboard(state: &AppState, ctx: &mut Context) -> Result<(), AppError> {
    let summary = stats::summary(&state.db, None, None).await?;
    let days = trend_dates();
    let trend = stats::summary(
        &state.db,
        Some(days.first().map(String::as_str).unwrap_or_default()),
        Some(days.last().map(String::as_str).unwrap_or_default()),
    )
    .await?;
    // 无阅读的日期补 0，保证折线图横轴完整覆盖近 30 天。
    let counts: HashMap<&str, i64> = trend
        .trend
        .iter()
        .map(|d| (d.date.as_str(), d.count))
        .collect();
    let trend_data: Vec<i64> = days
        .iter()
        .map(|d| counts.get(d.as_str()).copied().unwrap_or(0))
        .collect();

    let (drafts, _total) = posts_service::list_posts(
        &state.db,
        posts_service::PostListOptions {
            status: Some(PostStatus::Draft),
            post_type: Some(PostType::Post),
            category_slug: None,
            tag_slug: None,
            page: 1,
            page_size: DASHBOARD_DRAFT_LIMIT,
        },
    )
    .await?;

    ctx.insert(
        "stats",
        &json!({
            "total_posts": summary.total_posts,
            "total_moments": summary.total_moments,
            "total_attachments": summary.total_attachments,
            "total_views": summary.total_views,
        }),
    );
    ctx.insert(
        "drafts",
        &json!(
            drafts
                .iter()
                .map(|p| json!({
                    "id": p.id,
                    "title": p.title,
                    "updated_at": format_local(p.updated_at),
                }))
                .collect::<Vec<_>>()
        ),
    );
    // 内嵌 JSON 供 admin.js 画图：safe_string 标记避免 tera 自动转义破坏脚本。
    // 注意必须走 insert_value——insert 会重新序列化并丢失 safe 标记。
    let chart_json = json!({ "labels": days, "data": trend_data }).to_string();
    ctx.insert_value("chart_data", tera::Value::safe_string(&chart_json));
    Ok(())
}

/// 近 30 天日期序列（含今天，升序，UTC 日期）。
fn trend_dates() -> Vec<String> {
    let today = Utc::now().date_naive();
    (0..TREND_DAYS)
        .rev()
        .map(|i| {
            today
                .checked_sub_days(Days::new(i as u64))
                .expect("30 天内日期不会下溢")
                .format("%Y-%m-%d")
                .to_string()
        })
        .collect()
}

/// 后台时间展示：UTC → Asia/Shanghai（UTC+8）格式化 `YYYY-MM-DD HH:MM`。
pub(crate) fn format_local(dt: DateTime<Utc>) -> String {
    let tz = FixedOffset::east_opt(TZ_OFFSET_SECS).expect("UTC+8 偏移量合法");
    dt.with_timezone(&tz).format("%Y-%m-%d %H:%M").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nav_active_matches_by_path() {
        let nav = |path: &str| {
            admin_nav(path)
                .into_iter()
                .filter(|i| i.active)
                .map(|i| i.label)
                .collect::<Vec<_>>()
        };
        // 仪表盘仅精确命中 /admin（含结尾斜杠）
        assert_eq!(nav("/admin"), vec!["仪表盘"]);
        assert_eq!(nav("/admin/"), vec!["仪表盘"]);
        assert_eq!(nav("/admin/login"), Vec::<&str>::new());
        // 其余按前缀匹配
        assert_eq!(nav("/admin/stats"), vec!["统计"]);
        assert_eq!(nav("/admin/settings"), vec!["站点设置"]);
        // 前导相似路径不误伤
        assert_eq!(nav("/admin/setup"), Vec::<&str>::new());
        // 查看站点永不高亮
        assert_eq!(nav("/"), Vec::<&str>::new());
        assert_eq!(nav("/post/x"), Vec::<&str>::new());
    }

    #[test]
    fn layout_renders_active_nav() {
        let tera = build_tera();
        let mut ctx = Context::new();
        ctx.insert("site_name", "寒蝉 Hancic");
        ctx.insert("csrf", "token");
        ctx.insert("admin_nav", &nav_value(&admin_nav("/admin")));
        let html = tera.render("layout.html", &ctx).unwrap();
        assert!(
            html.contains(r#"class="admin-nav-item active"#),
            "仪表盘应高亮: {html}"
        );
    }
}
