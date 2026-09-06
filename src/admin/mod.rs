//! 后台管理路由：登录 / 登出 / 首启 setup / 仪表盘。
//!
//! T12 起页面统一渲染到 `assets/admin_templates/`（`include_str!` 编译期嵌入，
//! `build_tera` 注册进 `AppState::tera_admin`），`layout.html` 提供侧边栏导航
//! 与 CSRF meta。后台页全部附 `Cache-Control: no-store`（M19），渲染走 tera
//! 自动转义（M22：输出用户内容一律 `{{ }}`，脚本/HTML 不落地在模板里）。

use crate::error::AppError;
use crate::services::{settings as settings_service, stats as stats_service};
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
pub mod backup;
pub mod migrate;
pub mod moments;
pub mod system;
pub mod posts;
pub mod settings;
pub mod stats;
pub mod columns;
pub mod taxonomy;
pub mod themes;
pub mod tokens;
pub mod trails;

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
        .route("/moments/{id}/update", post(moments::update))
        .route("/attachments", get(attachments::list))
        .route("/attachments/{id}/delete", post(attachments::delete))
        .route("/attachments/upload", get(attachments::upload_page))
        .route("/api/attachments", get(attachments::api_list))
        .route("/taxonomy", get(taxonomy::list))
        .route("/taxonomy/categories", post(taxonomy::create_category))
        .route("/taxonomy/categories/{id}/update", post(taxonomy::update_category))
        .route("/taxonomy/categories/{id}/delete", post(taxonomy::delete_category))
        .route("/taxonomy/tags", post(taxonomy::create_tag))
        .route("/taxonomy/tags/{id}/delete", post(taxonomy::delete_tag))
        .route("/columns", get(columns::list).post(columns::create))
        .route("/columns/reorder", post(columns::reorder))
        .route("/columns/{id}", get(columns::detail))
        .route("/columns/{id}/update", post(columns::update))
        .route("/columns/{id}/delete", post(columns::delete))
        .route("/columns/{id}/posts/add", post(columns::add_post))
        .route("/columns/{id}/posts/remove", post(columns::remove_post))
        .route("/columns/{id}/posts/reorder", post(columns::reorder_posts))
        .route("/trails", get(trails::list))
        .route(
            "/trails/upload",
            post(trails::upload).layer(axum::extract::DefaultBodyLimit::max(
                trails::GPX_MAX_BYTES * trails::MAX_TRAIL_FILES + 1024 * 1024,
            )),
        )
        .route("/trails/{id}/update", post(trails::update))
        .route("/trails/{id}/delete", post(trails::delete))
        .route("/settings", get(settings::page))
        .route("/settings/save", post(settings::save))
        .route("/system", get(system::page))
        .route("/help", get(help_page))
        .route("/system/save", post(system::save))
        .route("/system/password", post(system::password))
        .route("/themes", get(themes::list))
        .route("/themes/import", post(themes::import))
        .route("/themes/{name}/activate", post(themes::activate))
        .route("/themes/{name}/uninstall", post(themes::uninstall))
        .route("/themes/{name}/preview", get(themes::preview))
        .route("/stats", get(admin_index))   // 历史路由兼容：统计已合并进仪表盘
        .route("/stats/clear", post(stats::clear))
        .route("/tokens", get(tokens::list).post(tokens::create))
        .route("/tokens/{id}/created", get(tokens::created_page))
        .route("/tokens/{id}/revoke", post(tokens::revoke))
        .route("/backup", get(backup::page))
        .route("/backup/export", post(backup::export))
        // restore 上传 zip 上限 500MB：路由层 DefaultBodyLimit（multipart 边界/字段头余量）
        .route(
            "/backup/restore",
            post(backup::restore).layer(axum::extract::DefaultBodyLimit::max(
                backup::RESTORE_MAX_BYTES as usize + 1024 * 1024,
            )),
        )
        // migrate 上传 zip 上限 500MB：同上
        .route(
            "/migrate",
            get(migrate::page)
                .post(migrate::run)
                .layer(axum::extract::DefaultBodyLimit::max(
                    migrate::MIGRATE_MAX_BYTES as usize + 1024 * 1024,
                )),
        )
}

/// 注册后台模板集：`include_str!` 编译期嵌入，全部为仓库内嵌模板，
/// `add_raw_templates` 仅做语法校验，理论上不会失败。
pub fn build_tera() -> Tera {
    let mut tera = Tera::default();
    tera.add_raw_templates(vec![
        ("layout.html", include_str!("../../assets/admin_templates/layout.html")),
        ("login_standalone.html", include_str!("../../assets/admin_templates/login_standalone.html")),
        ("setup.html", include_str!("../../assets/admin_templates/setup.html")),
        ("dashboard.html", include_str!("../../assets/admin_templates/dashboard.html")),
        ("posts_list.html", include_str!("../../assets/admin_templates/posts_list.html")),
        ("post_edit.html", include_str!("../../assets/admin_templates/post_edit.html")),
        ("moments.html", include_str!("../../assets/admin_templates/moments.html")),
        ("attachments.html", include_str!("../../assets/admin_templates/attachments.html")),
        ("taxonomy.html", include_str!("../../assets/admin_templates/taxonomy.html")),
        ("columns.html", include_str!("../../assets/admin_templates/columns.html")),
        ("column_posts.html", include_str!("../../assets/admin_templates/column_posts.html")),
        ("trails.html", include_str!("../../assets/admin_templates/trails.html")),
        ("settings.html", include_str!("../../assets/admin_templates/settings.html")),
        ("system.html", include_str!("../../assets/admin_templates/system.html")),
        ("themes.html", include_str!("../../assets/admin_templates/themes.html")),
        ("tokens.html", include_str!("../../assets/admin_templates/tokens.html")),
        ("tokens_created.html", include_str!("../../assets/admin_templates/tokens_created.html")),
        ("help.html", include_str!("../../assets/admin_templates/help.html")),
        ("backup.html", include_str!("../../assets/admin_templates/backup.html")),
        ("migrate.html", include_str!("../../assets/admin_templates/migrate.html")),
    ])
    .expect("内嵌后台模板注册失败");
    tera
}

/// 302 重定向（FOUND，与测试约定一致）。`base_path` 非空时给 Location 加子路径前缀
/// （部署在 subpath 时登录/后台跳转才能落在子路径下）。
pub(crate) fn redirect(base_path: &str, location: &str) -> Response {
    let loc = if base_path.is_empty() {
        location.to_string()
    } else {
        format!("{base_path}{location}")
    };
    (StatusCode::FOUND, [(header::LOCATION, loc)]).into_response()
}

/// 后台页基础上下文：site_name / site_logo / csrf / admin_nav（layout.html 消费）。
pub(crate) async fn base_ctx(state: &AppState, session: &Session, path: &str) -> (Context, String) {
    let csrf = session::csrf_token(session).await.unwrap_or_default();
    let settings = settings_service::get_many(
        &state.db,
        &["site_name", "site_logo"],
    )
    .await
    .unwrap_or_default();
    let site_name = settings
        .get("site_name")
        .map(String::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(&state.config.site_name);
    // 站点 Logo（上传路径如 /uploads/x.png）；未设置时布局用圆点占位
    let site_logo = settings
        .get("site_logo")
        .map(String::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("");
    let mut ctx = Context::new();
    ctx.insert("site_name", site_name);
    ctx.insert("site_logo", site_logo);
    ctx.insert("base_path", &state.config.base_path);
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
    /// 侧栏分组：content（内容管理）/ system（系统）
    group: &'static str,
    active: bool,
}

/// 侧边栏 12 个模块；active 按当前请求路径匹配。
fn admin_nav(path: &str) -> Vec<NavItem> {
    // (url, label, group)：内容管理 / 系统
    let items: [(&str, &str, &str); 13] = [
        ("/admin", "仪表盘", "dashboard"),
        ("/admin/posts", "文章", "content"),
        ("/admin/moments", "说说", "content"),
        ("/admin/attachments", "附件库", "content"),
        ("/admin/taxonomy", "分类标签", "content"),
        ("/admin/columns", "专栏管理", "content"),
        ("/admin/trails", "徒步轨迹", "content"),
        ("/admin/settings", "站点设置", "system"),
        ("/admin/themes", "主题管理", "system"),
        ("/admin/tokens", "API Token", "system"),
        ("/admin/backup", "备份恢复", "system"),
        ("/admin/system", "系统设置", "system"),
        ("/admin/help", "帮助文档", "system"),
    ];
    items
        .iter()
        .map(|(url, label, group)| NavItem {
            url,
            label,
            group,
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
    path.starts_with(url)
}

fn nav_value(items: &[NavItem]) -> Value {
    json!(items
        .iter()
        .map(|i| json!({ "url": i.url, "label": i.label, "group": i.group, "active": i.active }))
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
        Some("rate") => Some("尝试过于频繁，请 3 分钟后再试。"),
        Some("csrf") => Some("安全校验失败，请刷新页面后重试。"),
        Some(_) => Some("登录失败，请检查密码。"),
        None => None,
    };
    ctx.insert("has_password", &has_password);
    ctx.insert("error_tip", &error_tip.unwrap_or_default());
    // 独立登录页：不带后台外壳（侧栏/顶栏），全屏居中品牌卡片
    render_admin(&state, "login_standalone.html", &ctx)
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
        return redirect(&state.config.base_path, "/admin/login?error=rate");
    }
    // 2. CSRF 校验
    if session::verify_csrf(&session, form.csrf.as_deref())
        .await
        .is_err()
    {
        return redirect(&state.config.base_path, "/admin/login?error=csrf");
    }
    // 3. 校验密码并写入会话
    match session::login(&state.db, &session, &form.password).await {
        Ok(()) => {
            state.login_limiter.check(&ip, true); // 成功：清除失败记录
            redirect(&state.config.base_path, "/admin")
        }
        Err(_) => {
            state.login_limiter.record_failure(&ip);
            redirect(&state.config.base_path, "/admin/login?error=1")
        }
    }
}

async fn logout(State(state): State<AppState>, session: Session) -> Response {
    let _ = session::logout(&session).await;
    redirect(&state.config.base_path, "/admin/login")
}

async fn setup_page(
    State(state): State<AppState>,
    session: Session,
    Query(query): Query<HashMap<String, String>>,
    uri: OriginalUri,
) -> Response {
    if auth::has_password(&state.db).await.unwrap_or(true) {
        return redirect(&state.config.base_path, "/admin/login");
    }
    let (mut ctx, _csrf) = base_ctx(&state, &session, uri.path()).await;
    let error_tip = match query.get("error").map(String::as_str) {
        Some("csrf") => Some("安全校验失败，请刷新页面后重试。"),
        _ => None,
    };
    ctx.insert("error_tip", &error_tip.unwrap_or_default());
    ctx.insert("password_min_len", &auth::PASSWORD_MIN_LEN);
    render_admin(&state, "setup.html", &ctx)
}

async fn post_setup(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<SetupForm>,
) -> Response {
    if auth::has_password(&state.db).await.unwrap_or(true) {
        return redirect(&state.config.base_path, "/admin/login");
    }
    if session::verify_csrf(&session, form.csrf.as_deref())
        .await
        .is_err()
    {
        return redirect(&state.config.base_path, "/admin/setup?error=csrf");
    }
    match auth::set_password(&state.db, &form.password).await {
        Ok(()) => {
            // 自动登录
            if session
                .insert(session::USER_ID_KEY, session::ADMIN_ID)
                .await
                .is_err()
            {
                return redirect(&state.config.base_path, "/admin/setup?error=1");
            }
            redirect(&state.config.base_path, "/admin")
        }
        Err(_) => redirect(&state.config.base_path, "/admin/setup?error=1"),
    }
}

// ---------- 仪表盘 ----------

async fn admin_index(
    State(state): State<AppState>,
    session: Session,
    Query(query): Query<HashMap<String, String>>,
    uri: OriginalUri,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return redirect(&state.config.base_path, "/admin/login");
    }
    // 统计区间解析失败（非法日期等）回落到默认近 30 天，不阻塞仪表盘；
    // 统计时间语义按站点时区（settings.timezone）
    let tz = crate::services::timezone::site_timezone(&state.db).await;
    let (from, to) = stats::parse_range(&query, &tz).unwrap_or((None, None));
    let (mut ctx, _csrf) = base_ctx(&state, &session, uri.path()).await;
    if let Err(e) = fill_dashboard(&state, &mut ctx, &from, &to, &query, &tz).await {
        tracing::error!("仪表盘数据查询失败: {e:?}");
    }
    render_admin(&state, "dashboard.html", &ctx)
}

/// 仪表盘 Top 文章榜固定条数（区间内阅读降序；个人博客规模单次查询足够）。
const DASHBOARD_TOP_POSTS: i64 = 10;
/// 地区明细每页行数。
const REGION_PAGE_SIZE: usize = 10;

/// 仪表盘上下文：统计（卡片 + 区间趋势 + 文章排行 Top10 + 全球地区地图 + 跳转来源饼图）。
/// `from`/`to` 为有效区间（缺省近 30 天；双 None = 显式「全部」不设限），
/// `query` 提供地区明细分页（region_page）；趋势/范围按站点时区 `tz` 自然日。
async fn fill_dashboard(
    state: &AppState,
    ctx: &mut Context,
    from: &Option<String>,
    to: &Option<String>,
    query: &HashMap<String, String>,
    tz: &chrono_tz::Tz,
) -> Result<(), AppError> {
    // 「全部」（?range=all，parse_range 返回双 None）：表单留空 + 快捷钮高亮「全部」；
    // 其余用有效区间回填表单与趋势横轴
    let all = from.is_none() && to.is_none();
    let (from_str, to_str) = if all {
        (String::new(), String::new())
    } else {
        stats::effective_range(from, to, tz)
    };
    ctx.insert("from", &from_str);
    ctx.insert("to", &to_str);
    ctx.insert("all", &all);
    // 快捷按钮高亮：from/to 恰为 30/60/90 天窗口（含今天）时对应高亮；全部/自定义不高亮
    let today = Utc::now().with_timezone(tz).date_naive();
    let active_days = [30u32, 60, 90]
        .iter()
        .find_map(|d| {
            today
                .checked_sub_days(Days::new(u64::from(d - 1)))
                .map(|start| (start.format("%Y-%m-%d").to_string(), *d))
                .filter(|(s, _)| *s == from_str && today.format("%Y-%m-%d").to_string() == to_str)
                .map(|(_, d)| d)
        })
        .unwrap_or(0);
    ctx.insert("active_days", &active_days);

    // 卡片（total_views 随区间过滤，文章/说说/附件为全量）+ 趋势。
    // 横轴：全部=实际有阅读数据的日期序列；区间=逐日补 0。
    let summary =
        stats_service::summary(&state.db, from.as_deref(), to.as_deref(), tz).await?;
    let (labels, views) = if all {
        (
            summary.trend.iter().map(|d| d.date.clone()).collect::<Vec<_>>(),
            summary.trend.iter().map(|d| d.count).collect::<Vec<_>>(),
        )
    } else {
        let days = stats::date_range(&from_str, &to_str);
        let counts: HashMap<&str, i64> = summary
            .trend
            .iter()
            .map(|d| (d.date.as_str(), d.count))
            .collect();
        let views = days
            .iter()
            .map(|d| counts.get(d.as_str()).copied().unwrap_or(0))
            .collect();
        (days, views)
    };
    // 点赞趋势：与阅读趋势同横轴（UTC 日期），区间内无点赞补 0
    let like_daily = crate::services::likes::daily_like_count(
        &state.db,
        from.as_deref(),
        to.as_deref(),
        tz,
    )
    .await?;
    let like_counts: HashMap<&str, i64> = like_daily
        .iter()
        .map(|(date, count)| (date.as_str(), *count))
        .collect();
    let likes: Vec<i64> = labels
        .iter()
        .map(|d| like_counts.get(d.as_str()).copied().unwrap_or(0))
        .collect();

    // 文章排行：服务层 Top N（区间内阅读降序），固定展示前 10 不设分页
    let top = stats_service::top_posts(
        &state.db,
        from.as_deref(),
        to.as_deref(),
        DASHBOARD_TOP_POSTS,
        tz,
    )
    .await?;
    ctx.insert(
        "posts",
        &top
            .iter()
            .map(|(p, period)| {
                json!({
                    "id": p.id,
                    "title": p.title,
                    "views": p.views,
                    "period_views": period,
                })
            })
            .collect::<Vec<_>>(),
    );

    // 地区：按 (国家, 省份) 聚合，阅读降序。
    // 全量行注入地图 JSON（JS 按国家聚合着色全球地图）；表格展示当前页切片
    let regions =
        stats_service::by_region(&state.db, from.as_deref(), to.as_deref(), tz).await?;
    let region_rows = stats::region_view(&regions);
    let region_total_pages = region_rows.len().div_ceil(REGION_PAGE_SIZE).max(1);
    let region_page = query
        .get("region_page")
        .and_then(|p| p.parse::<usize>().ok())
        .filter(|&p| p > 0)
        .unwrap_or(1)
        .min(region_total_pages);
    let region_slice = region_rows
        .iter()
        .skip((region_page - 1) * REGION_PAGE_SIZE)
        .take(REGION_PAGE_SIZE)
        .cloned()
        .collect::<Vec<_>>();
    ctx.insert("regions", &region_slice);
    ctx.insert("region_page", &region_page);
    ctx.insert("region_total_pages", &region_total_pages);
    ctx.insert("region_total", &region_rows.len());
    ctx.insert("region_page_size", &REGION_PAGE_SIZE);
    // 地图数据（全量行；JS 侧按国家聚合 + 名称归一化匹配世界省界）
    ctx.insert_value(
        "region_data",
        tera::Value::safe_string(
            &serde_json::to_string(&region_rows).unwrap_or_else(|_| "[]".into()),
        ),
    );

    // 跳转来源：按 referer 平台分类分组，阅读降序（模板空态判断 + 饼图 JSON）
    let sources =
        stats_service::by_source(&state.db, from.as_deref(), to.as_deref(), tz).await?;
    let source_rows = stats::source_view(&sources);
    ctx.insert("sources", &source_rows);
    ctx.insert_value(
        "chart_sources",
        tera::Value::safe_string(
            &serde_json::to_string(&source_rows).unwrap_or_else(|_| "[]".into()),
        ),
    );

    let recent_like_count_7d = crate::services::likes::recent_like_count(&state.db, 7)
        .await
        .unwrap_or(0);
    ctx.insert(
        "stats",
        &json!({
            "total_posts": summary.total_posts,
            "total_moments": summary.total_moments,
            "total_attachments": summary.total_attachments,
            "total_views": summary.total_views,
            "recent_like_count_7d": recent_like_count_7d,
        }),
    );
    // 内嵌 JSON 供 admin.js 画图：safe_string 标记避免 tera 自动转义破坏脚本。
    // 注意必须走 insert_value——insert 会重新序列化并丢失 safe 标记。
    // 双数据集：views（阅读）+ likes（点赞），labels 为 UTC 日期横轴。
    let chart_json = json!({
        "labels": labels,
        "views": views,
        "likes": likes,
    })
    .to_string();
    ctx.insert_value("chart_data", tera::Value::safe_string(&chart_json));
    Ok(())
}

/// GET /admin/help：帮助页（REST API 使用说明 + MCP 集成）。
async fn help_page(
    State(state): State<AppState>,
    session: Session,
    uri: OriginalUri,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return redirect(&state.config.base_path, "/admin/login");
    }
    let (mut ctx, _csrf) = base_ctx(&state, &session, uri.path()).await;
    ctx.insert("api_base", &state.config.base_path);
    render_admin(&state, "help.html", &ctx)
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
        assert_eq!(nav("/admin/settings"), vec!["站点设置"]);
        assert_eq!(nav("/admin/stats"), Vec::<&str>::new()); // 统计已合并进仪表盘，无独立菜单
        // 前导相似路径不误伤
        assert_eq!(nav("/admin/setup"), Vec::<&str>::new());
        // 根路径无菜单项
        assert_eq!(nav("/"), Vec::<&str>::new());
        assert_eq!(nav("/post/x"), Vec::<&str>::new());
    }

    #[test]
    fn layout_renders_active_nav() {
        let tera = build_tera();
        let mut ctx = Context::new();
        ctx.insert("site_name", "寒蝉 Hancic");
        ctx.insert("site_logo", "");
        ctx.insert("base_path", "");
        ctx.insert("csrf", "token");
        ctx.insert("admin_nav", &nav_value(&admin_nav("/admin")));
        let html = tera.render("layout.html", &ctx).unwrap();
        assert!(
            html.contains(r#"class="admin-nav-item active"#),
            "仪表盘应高亮: {html}"
        );
    }
}
