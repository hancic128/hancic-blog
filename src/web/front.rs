//! 前台页面：文章流、文章/独立页、分类/标签归档、搜索、静态资源与错误页。
//!
//! 页面经 `site_context` 注入站点信息（settings 表），模板取自
//! `themes/<active_theme>/templates/`（T6 `build_tera` 构建、注册
//! `markdown`/`date` 过滤器）。渲染失败统一输出 `error.html`（含状态码）。

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::models::{Post, PostStatus, PostType};
use crate::services::{posts, settings, taxonomy};
use crate::AppState;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{Request, StatusCode, Uri, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use tera::Context;
use tower::ServiceExt;
use tower_http::services::ServeDir;

/// 列表页每页文章数。
const PAGE_SIZE: i64 = 10;
/// 静态资源缓存头：7 天（T17 允许配置后调整）。
const STATIC_CACHE: &str = "public, max-age=604800";

/// 前台路由（挂在 `/`），静态资源挂载点：
/// - `/uploads/{*path}` 附件目录
/// - `/theme/{name}/static/{*path}` 主题静态资源
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(index))
        .route("/post/{slug}", get(post_page))
        .route("/page/{slug}", get(page_page))
        .route("/category/{slug}", get(category_page))
        .route("/tag/{slug}", get(tag_page))
        .route("/about", get(about_page))
        .route("/search", get(search_page))
        .route("/uploads/{*path}", get(serve_uploads))
        .route("/theme/{name}/static/{*path}", get(serve_theme_static))
}

/// 站点信息上下文：`site` 对象含 name/desc/nav/social/active_theme/mode。
/// nav/social 来自 settings 的 JSON 字符串，解析为 tera 可迭代对象。
pub async fn site_context(db: &Db) -> AppResult<Context> {
    let s = settings::get_many(
        db,
        &[
            "site_name",
            "site_desc",
            "site_nav",
            "site_social",
            "active_theme",
            "theme_mode",
        ],
    )
    .await?;
    let mut ctx = Context::new();
    ctx.insert(
        "site",
        &json!({
            "name": s.get("site_name").map(String::as_str).unwrap_or("寒蝉 Hancic"),
            "desc": s.get("site_desc").map(String::as_str).unwrap_or(""),
            "nav": parse_json_array(s.get("site_nav").map(String::as_str).unwrap_or("[]")),
            "social": parse_json_array(s.get("site_social").map(String::as_str).unwrap_or("{}")),
            "active_theme": s.get("active_theme").map(String::as_str).unwrap_or("default"),
            "mode": s.get("theme_mode").map(String::as_str).unwrap_or("auto"),
        }),
    );
    Ok(ctx)
}

/// 解析 settings 中的 JSON 数组/对象字符串；非法时回退空值。
fn parse_json_array(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or_else(|_| json!([]))
}

// ---------- 页面 handler ----------

async fn index(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let page = page_param(&query);
    match listing_ctx(&state.db, page, None, None).await {
        Ok(ctx) => render(&state, "index.html", &ctx).await,
        Err(e) => render_error(&state, e).await,
    }
}

async fn post_page(State(state): State<AppState>, Path(slug): Path<String>) -> Response {
    let out = async {
        let post = posts::get_post_by_slug(&state.db, &slug)
            .await?
            .filter(|p| p.status == PostStatus::Published && p.post_type == PostType::Post)
            .ok_or_else(|| AppError::NotFound("文章不存在".into()))?;
        let (prev, next) = posts::adjacent_posts(&state.db, &post).await?;
        let tags = posts::list_tags_of_post(&state.db, post.id).await?;
        let category = category_of(&state.db, post.category_id).await?;
        let mut ctx = site_context(&state.db).await?;
        ctx.insert("post", &post_value(&post));
        ctx.insert(
            "category",
            &category
                .map(|c| json!({ "slug": c.slug, "name": c.name }))
                .unwrap_or(json!(null)),
        );
        ctx.insert(
            "tags",
            &json!(
                tags.iter()
                    .map(|t| json!({ "slug": t.slug, "name": t.name }))
                    .collect::<Vec<_>>()
            ),
        );
        ctx.insert("prev", &adjacent_value(prev.as_ref()));
        ctx.insert("next", &adjacent_value(next.as_ref()));
        Ok::<_, AppError>(ctx)
    }
    .await;
    match out {
        Ok(ctx) => render(&state, "post.html", &ctx).await,
        Err(e) => render_error(&state, e).await,
    }
}

async fn page_page(State(state): State<AppState>, Path(slug): Path<String>) -> Response {
    let out = async {
        let page = posts::get_post_by_slug(&state.db, &slug)
            .await?
            .filter(|p| p.status == PostStatus::Published && p.post_type == PostType::Page)
            .ok_or_else(|| AppError::NotFound("页面不存在".into()))?;
        let mut ctx = site_context(&state.db).await?;
        ctx.insert("page", &post_value(&page));
        Ok::<_, AppError>(ctx)
    }
    .await;
    match out {
        Ok(ctx) => render(&state, "page.html", &ctx).await,
        Err(e) => render_error(&state, e).await,
    }
}

/// `/about` 快捷路由 → `/page/about`；不存在则 404。
async fn about_page(State(state): State<AppState>) -> Response {
    let out = async {
        let page = posts::get_post_by_slug(&state.db, "about")
            .await?
            .filter(|p| p.status == PostStatus::Published && p.post_type == PostType::Page)
            .ok_or_else(|| AppError::NotFound("关于页面不存在".into()))?;
        let mut ctx = site_context(&state.db).await?;
        ctx.insert("page", &post_value(&page));
        Ok::<_, AppError>(ctx)
    }
    .await;
    match out {
        Ok(ctx) => render(&state, "page.html", &ctx).await,
        Err(e) => render_error(&state, e).await,
    }
}

async fn category_page(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let out = async {
        let category = taxonomy::get_category_by_slug(&state.db, &slug)
            .await?
            .ok_or_else(|| AppError::NotFound("分类不存在".into()))?;
        let mut ctx = listing_ctx(&state.db, page_param(&query), Some(slug), None).await?;
        ctx.insert(
            "category",
            &json!({ "slug": category.slug, "name": category.name }),
        );
        Ok::<_, AppError>(ctx)
    }
    .await;
    match out {
        Ok(ctx) => render(&state, "category.html", &ctx).await,
        Err(e) => render_error(&state, e).await,
    }
}

async fn tag_page(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let out = async {
        let tag = taxonomy::list_tags(&state.db)
            .await?
            .into_iter()
            .find(|t| t.slug == slug)
            .ok_or_else(|| AppError::NotFound("标签不存在".into()))?;
        let mut ctx = listing_ctx(&state.db, page_param(&query), None, Some(slug)).await?;
        ctx.insert("tag", &json!({ "slug": tag.slug, "name": tag.name }));
        Ok::<_, AppError>(ctx)
    }
    .await;
    match out {
        Ok(ctx) => render(&state, "tag.html", &ctx).await,
        Err(e) => render_error(&state, e).await,
    }
}

/// 搜索页：本任务仅渲染表单与空结果，T9 接入 FTS5 全文检索。
async fn search_page(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let q = query.get("q").cloned().unwrap_or_default();
    let mut ctx = match site_context(&state.db).await {
        Ok(c) => c,
        Err(e) => return render_error(&state, e).await,
    };
    ctx.insert("search_query", &q);
    ctx.insert("posts", &json!([]));
    render(&state, "search.html", &ctx).await
}

/// 未匹配路由 → 404 错误页。
pub async fn not_found(State(state): State<AppState>) -> Response {
    render_error(&state, AppError::NotFound("页面不存在".into())).await
}

// ---------- 静态资源 ----------

async fn serve_uploads(State(state): State<AppState>, req: Request<Body>) -> Response {
    serve_from(state.config.data_dir.join("uploads"), "/uploads", req).await
}

async fn serve_theme_static(
    State(state): State<AppState>,
    Path((name, _path)): Path<(String, String)>,
    req: Request<Body>,
) -> Response {
    if !is_valid_theme_name(&name) {
        return render_error(&state, AppError::NotFound("资源不存在".into())).await;
    }
    let base = state.config.data_dir.join("themes").join(&name).join("static");
    serve_from(base, &format!("/theme/{name}/static"), req).await
}

/// 主题名白名单：仅 ASCII 字母数字与 `-`/`_`，防止路径穿越。
fn is_valid_theme_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// 去掉挂载前缀后用 ServeDir 服务目录，并附加缓存头。
async fn serve_from(base: PathBuf, prefix: &str, req: Request<Body>) -> Response {
    let uri = match strip_prefix_uri(req.uri(), prefix) {
        Some(u) => u,
        None => return (StatusCode::NOT_FOUND, "not found").into_response(),
    };
    let mut req = req;
    *req.uri_mut() = uri;
    match ServeDir::new(&base).oneshot(req).await {
        Ok(mut res) => {
            res.headers_mut().insert(
                header::CACHE_CONTROL,
                header::HeaderValue::from_static(STATIC_CACHE),
            );
            res.map(Body::new)
        }
        Err(never) => match never {},
    }
}

/// 重写请求 URI：去掉静态资源挂载前缀，仅保留其后路径与查询。
fn strip_prefix_uri(uri: &Uri, prefix: &str) -> Option<Uri> {
    let rest = uri.path().strip_prefix(prefix)?;
    let rest = if rest.is_empty() { "/" } else { rest };
    let path_and_query = match uri.query() {
        Some(q) => format!("{rest}?{q}"),
        None => rest.to_string(),
    };
    Uri::builder().path_and_query(path_and_query).build().ok()
}

// ---------- 上下文构造 ----------

/// 读取 `page` 查询参数，非法或缺失时取 1。
fn page_param(query: &HashMap<String, String>) -> i64 {
    query
        .get("page")
        .and_then(|p| p.parse::<i64>().ok())
        .unwrap_or(1)
        .max(1)
}

/// 文章流列表上下文（首页/分类/标签共用）：注入 posts + pagination。
async fn listing_ctx(
    db: &Db,
    page: i64,
    category_slug: Option<String>,
    tag_slug: Option<String>,
) -> AppResult<Context> {
    let (items, total) = posts::list_posts(
        db,
        posts::PostListOptions {
            status: Some(PostStatus::Published),
            category_slug,
            tag_slug,
            page,
            page_size: PAGE_SIZE,
        },
    )
    .await?;
    let mut ctx = site_context(db).await?;
    ctx.insert("posts", &post_list_value(&items));
    ctx.insert("pagination", &pagination_value(page, total));
    Ok(ctx)
}

/// 列表页文章 JSON：标题/日期/excerpt/阅读量/链接。
fn post_list_value(items: &[Post]) -> Value {
    json!(items
        .iter()
        .map(|p| json!({
            "slug": p.slug,
            "title": p.title,
            "excerpt": p.excerpt,
            "published_at": p.published_at.map(|d| d.to_rfc3339()),
            "views": p.views,
            "url": post_url(p),
        }))
        .collect::<Vec<_>>())
}

/// 文章页上下文 JSON。
fn post_value(p: &Post) -> Value {
    json!({
        "slug": p.slug,
        "title": p.title,
        "content_md": p.content_md,
        "excerpt": p.excerpt,
        "published_at": p.published_at.map(|d| d.to_rfc3339()),
        "views": p.views,
    })
}

/// 文章链接：独立页走 `/page/`，普通文章走 `/post/`。
fn post_url(p: &Post) -> String {
    if p.post_type == PostType::Page {
        format!("/page/{}", p.slug)
    } else {
        format!("/post/{}", p.slug)
    }
}

/// 上一篇/下一篇 JSON；无则为 null。
fn adjacent_value(p: Option<&Post>) -> Value {
    match p {
        Some(p) => json!({ "slug": p.slug, "title": p.title }),
        None => json!(null),
    }
}

fn pagination_value(page: i64, total: i64) -> Value {
    let total_pages = (total + PAGE_SIZE - 1) / PAGE_SIZE;
    json!({
        "current": page,
        "total": total,
        "total_pages": total_pages,
        "prev": (page > 1).then_some(page - 1),
        "next": (page < total_pages).then_some(page + 1),
    })
}

/// 文章所属分类（按 id 查找）。
async fn category_of(db: &Db, id: Option<i64>) -> AppResult<Option<crate::models::Category>> {
    match id {
        Some(id) => Ok(taxonomy::list_categories(db)
            .await?
            .into_iter()
            .find(|c| c.id == id)),
        None => Ok(None),
    }
}

// ---------- 渲染 ----------

/// 渲染模板；模板缺失/出错时回退 500 错误页。
async fn render(state: &AppState, template: &str, ctx: &Context) -> Response {
    match state.tera.render(template, ctx) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("渲染模板 {template} 失败: {e}");
            render_error(state, AppError::Internal("页面渲染失败".into())).await
        }
    }
}

/// 渲染错误页（404/500 等）：`error.html`，模板不可用时回退内联 HTML。
async fn render_error(state: &AppState, err: AppError) -> Response {
    let status = err.status();
    let message = err.message().to_string();
    let mut ctx = match site_context(&state.db).await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("构建错误页上下文失败: {e:?}");
            return fallback_error_page(status, &message);
        }
    };
    ctx.insert(
        "error",
        &json!({ "code": status.as_u16(), "message": message }),
    );
    match state.tera.render("error.html", &ctx) {
        Ok(html) => (status, Html(html)).into_response(),
        Err(e) => {
            tracing::error!("渲染 error.html 失败: {e}");
            fallback_error_page(status, &message)
        }
    }
}

/// 错误页最后兜底：纯内联 HTML。
fn fallback_error_page(status: StatusCode, message: &str) -> Response {
    let code = status.as_u16();
    let reason = status.canonical_reason().unwrap_or("");
    (
        status,
        Html(format!(
            "<!DOCTYPE html><html lang=\"zh-CN\"><meta charset=\"utf-8\">\
             <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
             <title>{code} {reason}</title>\
             <body style=\"font:17px/1.8 -apple-system,'PingFang SC',sans-serif;max-width:720px;margin:4rem auto;padding:0 1.25rem\">\
             <h1>{code} {reason}</h1><p>{message}</p>\
             <p><a href=\"/\">返回首页</a></p></body></html>"
        )),
    )
        .into_response()
}
