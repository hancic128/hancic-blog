//! 前台页面：文章流、文章/独立页、分类/标签归档、搜索、静态资源与错误页。
//!
//! 页面经 `site_context` 注入站点信息（settings 表），模板取自
//! `themes/<active_theme>/templates/`（T6 `build_tera` 构建、注册
//! `markdown`/`date` 过滤器）。渲染失败统一输出 `error.html`（含状态码）。
//! 支持后台主题预览（T18）：`?theme_preview={name}` 只读覆盖
//! `site.active_theme` 与渲染用 tera（每次请求按预览主题构建），不落库。

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::models::{Moment, Post, PostStatus, PostType};
use crate::services::{moments, posts, settings, stats, taxonomy};
use crate::{themes, AppState};
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Request, StatusCode, Uri, header};
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
/// 说说页每页条数。
const MOMENTS_PAGE_SIZE: i64 = 20;
/// 静态资源缓存头：7 天（T17 允许配置后调整）。
const STATIC_CACHE: &str = "public, max-age=604800";

/// 前台路由（挂在 `/`），静态资源挂载点：
/// - `/uploads/{*path}` 附件目录
/// - `/theme/{name}/static/{*path}` 主题静态资源
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(index))
        .route("/archives", get(archives_page))
        .route("/post/{slug}", get(post_page))
        .route("/page/{slug}", get(page_page))
        .route("/category/{slug}", get(category_page))
        .route("/tag/{slug}", get(tag_page))
        .route("/about", get(about_page))
        .route("/moments", get(moments_page))
        .route("/search", get(search_page))
        .route("/uploads/{*path}", get(serve_uploads))
        .route("/theme/{name}/static/{*path}", get(serve_theme_static))
}

/// 站点信息上下文：`site` 对象含 name/desc/nav/social/active_theme/mode。
/// `preview` 为预览主题名（后台 `?theme_preview=`）：仅覆盖 `active_theme`
/// 用于本次只读渲染，不写库；模板里静态资源路径
/// `/theme/{{ site.active_theme }}/static/...` 随之指向预览主题。
pub async fn site_context(db: &Db, base: &str, preview: Option<String>) -> AppResult<Context> {
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
    // 预览覆盖 active_theme；缺省回退 settings，再回退默认主题
    let active_theme = preview.unwrap_or_else(|| {
        s.get("active_theme")
            .cloned()
            .unwrap_or_else(|| "default".to_string())
    });
    let mut ctx = Context::new();
    ctx.insert("base_path", base);
    let categories = taxonomy::list_categories(db).await?;
    ctx.insert(
        "site",
        &json!({
            "name": s.get("site_name").map(String::as_str).unwrap_or("寒蝉 Hancic"),
            "desc": s.get("site_desc").map(String::as_str).unwrap_or(""),
            "nav": parse_json_array(s.get("site_nav").map(String::as_str).unwrap_or("[]")),
            "social": parse_json_array(s.get("site_social").map(String::as_str).unwrap_or("{}")),
            "active_theme": active_theme,
            "mode": s.get("theme_mode").map(String::as_str).unwrap_or("auto"),
            "categories": categories.iter().map(|c| json!({ "slug": c.slug, "name": c.name })).collect::<Vec<_>>(),
        }),
    );
    Ok(ctx)
}

/// 解析 settings 中的 JSON 数组/对象字符串；非法时回退空值。
fn parse_json_array(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or_else(|_| json!([]))
}

/// 解析 `?theme_preview=`：名字合法且主题存在（theme.toml 可读）才生效，
/// 否则回退默认渲染——预览参数既不破坏页面，也不暴露不存在的主题。
fn resolve_preview(state: &AppState, query: &HashMap<String, String>) -> Option<String> {
    let name = query.get("theme_preview")?;
    if !themes::is_valid_name(name) {
        return None;
    }
    themes::load_meta(&state.config.data_dir.join("themes"), name).ok()?;
    Some(name.clone())
}

// ---------- 页面 handler ----------

async fn index(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let preview = resolve_preview(&state, &query);
    let out = async {
        // 更新日历（近 53 周，含发布/更新/说说详情）与最近说说（5 条）
        let calendar = posts::activity_calendar(&state.db, 371).await?;
        let (moments, _total) = moments::list_moments(&state.db, None, false, None, 1, 5).await?;
        // 最近文章（首页仅展示 5 篇，完整列表走 /archives）
        let (items, total) = posts::list_posts(
            &state.db,
            posts::PostListOptions {
                status: Some(PostStatus::Published),
                post_type: Some(PostType::Post),
                category_slug: None,
                tag_slug: None,
                month: None,
                sort: None,
                page: 1,
                page_size: 5,
            },
        )
        .await?;
        let mut ctx = site_context(&state.db, &state.config.base_path, preview.clone()).await?;
        ctx.insert("calendar", &calendar_matrix(&calendar));
        ctx.insert("moments", &moment_list_value(&moments));
        ctx.insert("posts", &post_list_value(&state.db, &state.config.base_path, &items).await?);
        ctx.insert("post_total", &total);
        Ok::<_, AppError>(ctx)
    }
    .await;
    match out {
        Ok(ctx) => render(&state, "index.html", &ctx, preview.as_deref()).await,
        Err(e) => render_error(&state, e).await,
    }
}

/// 文章归档页：全部已发布文章分页列表 + 顶部标签云；支持 `?month=YYYY-MM` 月份筛选。
async fn archives_page(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let page = page_param(&query);
    let month = query.get("month").filter(|m| !m.is_empty()).cloned();
    let preview = resolve_preview(&state, &query);
    let out = async {
        let mut ctx = listing_ctx(&state.db, &state.config.base_path, page, None, None, month.clone(), preview.clone()).await?;
        let tags = taxonomy::list_tags(&state.db).await?;
        let months = posts::month_list(&state.db).await?;
        ctx.insert(
            "all_tags",
            &json!(tags
                .iter()
                .map(|t| json!({ "slug": t.slug, "name": t.name }))
                .collect::<Vec<_>>()),
        );
        ctx.insert(
            "months",
            &json!(months.iter().map(|m| json!({ "month": m })).collect::<Vec<_>>()),
        );
        ctx.insert("current_month", &month);
        let month_base = format!("{}/archives", state.config.base_path);
        ctx.insert("month_base", &month_base);
        Ok::<_, AppError>(ctx)
    }
    .await;
    match out {
        Ok(ctx) => render(&state, "archives.html", &ctx, preview.as_deref()).await,
        Err(e) => render_error(&state, e).await,
    }
}

async fn post_page(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let preview = resolve_preview(&state, &query);
    let out = async {
        let post = posts::get_post_by_slug(&state.db, &slug)
            .await?
            .filter(|p| p.status == PostStatus::Published && p.post_type == PostType::Post)
            .ok_or_else(|| AppError::NotFound("文章不存在".into()))?;
        let (prev, next) = posts::adjacent_posts(&state.db, &post).await?;
        let tags = posts::list_tags_of_post(&state.db, post.id).await?;
        let category = category_of(&state.db, post.category_id).await?;
        let mut ctx = site_context(&state.db, &state.config.base_path, preview.clone()).await?;
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
        // 正文（带标题锚点）+ 1~3 级目录，供右侧栏导航
        let (content_html, toc) = crate::markdown::render_with_toc(&post.content_md);
        ctx.insert("content_html", &content_html);
        ctx.insert(
            "toc",
            &json!(toc
                .iter()
                .map(|t| json!({ "level": t.level, "text": t.text, "id": format!("toc-{}", t.id) }))
                .collect::<Vec<_>>()),
        );
        record_view_once(&state, &post, &headers).await;
        Ok::<_, AppError>(ctx)
    }
    .await;
    match out {
        Ok(ctx) => render(&state, "post.html", &ctx, preview.as_deref()).await,
        Err(e) => render_error(&state, e).await,
    }
}

/// 记一次阅读：优先 `x-real-ip`（nginx 反代），其次 `x-forwarded-for` 首段；
/// 失败仅告警，不阻断页面渲染。
async fn record_view_once(state: &AppState, post: &Post, headers: &HeaderMap) {
    let ip = headers
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .or_else(|| {
            headers
                .get("x-forwarded-for")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.split(',').next())
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_default();
    let ua = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let referer = headers
        .get("referer")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    if let Err(e) = stats::record_view(
        &state.db,
        post.id,
        &ip,
        &ua,
        &referer,
        &state.ip_searcher,
    )
    .await
    {
        tracing::warn!("记录阅读失败 post_id={}: {e:?}", post.id);
    }
}

async fn page_page(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let preview = resolve_preview(&state, &query);
    let out = async {
        let page = posts::get_post_by_slug(&state.db, &slug)
            .await?
            .filter(|p| p.status == PostStatus::Published && p.post_type == PostType::Page)
            .ok_or_else(|| AppError::NotFound("页面不存在".into()))?;
        let mut ctx = site_context(&state.db, &state.config.base_path, preview.clone()).await?;
        ctx.insert("page", &post_value(&page));
        Ok::<_, AppError>(ctx)
    }
    .await;
    match out {
        Ok(ctx) => render(&state, "page.html", &ctx, preview.as_deref()).await,
        Err(e) => render_error(&state, e).await,
    }
}

/// `/about` 快捷路由 → `/page/about`；不存在则 404。
async fn about_page(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let preview = resolve_preview(&state, &query);
    let out = async {
        let page = posts::get_post_by_slug(&state.db, "about")
            .await?
            .filter(|p| p.status == PostStatus::Published && p.post_type == PostType::Page)
            .ok_or_else(|| AppError::NotFound("关于页面不存在".into()))?;
        let mut ctx = site_context(&state.db, &state.config.base_path, preview.clone()).await?;
        ctx.insert("page", &post_value(&page));
        Ok::<_, AppError>(ctx)
    }
    .await;
    match out {
        Ok(ctx) => render(&state, "page.html", &ctx, preview.as_deref()).await,
        Err(e) => render_error(&state, e).await,
    }
}

/// 说说页：朋友圈式按天分组展示（分页 20/页）；右侧按月份筛选（默认半年）。
async fn moments_page(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let page = page_param(&query);
    let month = query.get("month").filter(|m| !m.is_empty()).cloned();
    let preview = resolve_preview(&state, &query);
    let out = async {
        let (items, total) =
            moments::list_moments(&state.db, month.as_deref(), false, None, page, MOMENTS_PAGE_SIZE).await?;
        let mut ctx = site_context(&state.db, &state.config.base_path, preview.clone()).await?;
        // 与首页最近说说同款时间线折叠：默认单行，点击展开全文与附件
        ctx.insert("moments", &moment_items_value(&state.db, &state.config.base_path, &items).await?);
        ctx.insert("pagination", &moments_pagination_value(page, total));
        let months = moments::month_list(&state.db).await?;
        ctx.insert(
            "months",
            &json!(months.iter().map(|m| json!({ "month": m })).collect::<Vec<_>>()),
        );
        ctx.insert("current_month", &month);
        let month_base = format!("{}/moments", state.config.base_path);
        ctx.insert("month_base", &month_base);
        Ok::<_, AppError>(ctx)
    }
    .await;
    match out {
        Ok(ctx) => render(&state, "moments.html", &ctx, preview.as_deref()).await,
        Err(e) => render_error(&state, e).await,
    }
}

async fn category_page(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let preview = resolve_preview(&state, &query);
    let out = async {
        let category = taxonomy::get_category_by_slug(&state.db, &slug)
            .await?
            .ok_or_else(|| AppError::NotFound("分类不存在".into()))?;
        let mut ctx =
            listing_ctx(&state.db, &state.config.base_path, page_param(&query), Some(slug), None, None, preview.clone()).await?;
        ctx.insert(
            "category",
            &json!({ "slug": category.slug, "name": category.name }),
        );
        // 该分类下已发布文章使用的标签（去重），供"分类：XX 下方标签云"
        let category_tags = taxonomy::tags_of_category(&state.db, category.id).await?;
        ctx.insert(
            "category_tags",
            &json!(category_tags
                .iter()
                .map(|t| json!({ "slug": t.slug, "name": t.name }))
                .collect::<Vec<_>>()),
        );
        Ok::<_, AppError>(ctx)
    }
    .await;
    match out {
        Ok(ctx) => render(&state, "category.html", &ctx, preview.as_deref()).await,
        Err(e) => render_error(&state, e).await,
    }
}

async fn tag_page(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let preview = resolve_preview(&state, &query);
    let out = async {
        let tag = taxonomy::list_tags(&state.db)
            .await?
            .into_iter()
            .find(|t| t.slug == slug)
            .ok_or_else(|| AppError::NotFound("标签不存在".into()))?;
        let month = query.get("month").filter(|m| !m.is_empty()).cloned();
        let mut ctx = listing_ctx(
            &state.db,
            &state.config.base_path,
            page_param(&query),
            None,
            Some(slug.clone()),
            month.clone(),
            preview.clone(),
        )
        .await?;
        ctx.insert("tag", &json!({ "slug": tag.slug, "name": tag.name }));
        // 该标签下文章的月份，供右侧栏按月份筛选
        let months = posts::month_list_filtered(&state.db, None, Some(&slug)).await?;
        ctx.insert(
            "months",
            &json!(months.iter().map(|m| json!({ "month": m })).collect::<Vec<_>>()),
        );
        ctx.insert("current_month", &month);
        let month_base = format!("{}/tag/{slug}", state.config.base_path);
        ctx.insert("month_base", &month_base);
        Ok::<_, AppError>(ctx)
    }
    .await;
    match out {
        Ok(ctx) => render(&state, "tag.html", &ctx, preview.as_deref()).await,
        Err(e) => render_error(&state, e).await,
    }
}

/// 搜索页：FTS5 全文检索，支持分页（分页链接保留 q）。
async fn search_page(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let q = query.get("q").cloned().unwrap_or_default();
    let page = page_param(&query);
    let preview = resolve_preview(&state, &query);
    let out = async {
        let (hits, total) = posts::search_posts(&state.db, &q, page, PAGE_SIZE).await?;
        let mut ctx = site_context(&state.db, &state.config.base_path, preview.clone()).await?;
        ctx.insert("search_query", &q);
        ctx.insert("posts", &search_hit_list_value(&state.config.base_path, &hits));
        ctx.insert("pagination", &search_pagination_value(&state.config.base_path, page, total, &q));
        Ok::<_, AppError>(ctx)
    }
    .await;
    match out {
        Ok(ctx) => render(&state, "search.html", &ctx, preview.as_deref()).await,
        Err(e) => render_error(&state, e).await,
    }
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
    if !themes::is_valid_name(&name) {
        return render_error(&state, AppError::NotFound("资源不存在".into())).await;
    }
    let base = state.config.data_dir.join("themes").join(&name).join("static");
    serve_from(base, &format!("/theme/{name}/static"), req).await
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
            // 缓存头只加在成功响应上（M27）：404/错误响应不应被浏览器/中间层缓存
            if res.status().is_success() {
                res.headers_mut().insert(
                    header::CACHE_CONTROL,
                    header::HeaderValue::from_static(STATIC_CACHE),
                );
            }
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
    base: &str,
    page: i64,
    category_slug: Option<String>,
    tag_slug: Option<String>,
    month: Option<String>,
    preview: Option<String>,
) -> AppResult<Context> {
    let (items, total) = posts::list_posts(
        db,
        posts::PostListOptions {
            status: Some(PostStatus::Published),
            post_type: Some(PostType::Post),
            category_slug,
            tag_slug,
            month,
            sort: None,
            page,
            page_size: PAGE_SIZE,
        },
    )
    .await?;
    let mut ctx = site_context(db, base, preview).await?;
    ctx.insert("posts", &post_list_value(db, base, &items).await?);
    ctx.insert("pagination", &pagination_value(page, total));
    Ok(ctx)
}

/// 列表页文章 JSON：标题/日期/excerpt/阅读量/链接。
async fn post_list_value(db: &Db, base: &str, items: &[Post]) -> AppResult<Value> {
    let mut list = Vec::with_capacity(items.len());
    for p in items {
        let tags = posts::list_tags_of_post(db, p.id).await?;
        list.push(json!({
            "slug": p.slug,
            "title": p.title,
            "excerpt": p.excerpt,
            "published_at": p.published_at.map(|d| d.to_rfc3339()),
            "views": p.views,
            "url": post_url(base, p),
            "tags": tags.iter().map(|t| json!({ "slug": t.slug, "name": t.name })).collect::<Vec<_>>(),
        }));
    }
    Ok(json!(list))
}

/// 搜索页命中 JSON：标题/链接/高亮片段（`| safe` 渲染 `<mark>`）/日期/阅读量。
fn search_hit_list_value(base: &str, hits: &[posts::SearchHit]) -> Value {
    json!(hits
        .iter()
        .map(|h| json!({
            "title": h.post.title,
            "snippet": h.snippet,
            "published_at": h.post.published_at.map(|d| d.to_rfc3339()),
            "views": h.post.views,
            "url": post_url(base, &h.post),
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

/// 文章链接：独立页走 `/page/`，普通文章走 `/post/`；`base` 为部署子路径前缀。
fn post_url(base: &str, p: &Post) -> String {
    let path = if p.post_type == PostType::Page {
        format!("/page/{}", p.slug)
    } else {
        format!("/post/{}", p.slug)
    };
    format!("{base}{path}")
}

/// 更新日历单日条目：(日期, 动态数, [(类型, 标题)])。
type CalendarDay = (String, i64, Vec<(String, String)>);
/// 日历查询表键/值别名。
type DayKey<'a> = &'a str;
type DayVal<'a> = (i64, &'a [(String, String)]);

/// GitHub 风格更新日历矩阵：53 周 × 7 天。
/// 外层数组 = 周（列），内层 = 该周 7 天（行，周日→周六）。
/// 每格：`{ date, count, level, tip }`，tip 为多行 tooltip（日期 + 动态标题）。
fn calendar_matrix(calendar: &[CalendarDay]) -> Value {
    use std::collections::HashMap;
    let map: HashMap<DayKey<'_>, DayVal<'_>> = calendar
        .iter()
        .map(|(d, c, items)| (d.as_str(), (*c, items.as_slice())))
        .collect();
    let today = chrono::Utc::now().date_naive();
    let start = today - chrono::Days::new(370);
    let mut weeks: Vec<Vec<Value>> = Vec::new();
    let mut week: Vec<Value> = Vec::new();
    for i in 0..371 {
        let day = start + chrono::Days::new(i);
        let key = day.format("%Y-%m-%d").to_string();
        let (count, items) = map.get(key.as_str()).copied().unwrap_or((0, &[]));
        let level = match count {
            0 => 0,
            1 => 1,
            2..=3 => 2,
            4..=6 => 3,
            _ => 4,
        };
        let mut tip = key.clone();
        for (kind, title) in items {
            let label = match kind.as_str() {
                "post" => "发布",
                "update" => "更新",
                _ => "说说",
            };
            tip.push_str(&format!("\n{label}：{title}"));
        }
        week.push(json!({ "date": key, "count": count, "level": level, "tip": tip }));
        if week.len() == 7 {
            weeks.push(std::mem::take(&mut week));
        }
    }
    if !week.is_empty() {
        weeks.push(week);
    }
    json!(weeks)
}

/// 说说列表 JSON（首页"最近说说"用）。
fn moment_list_value(moments: &[Moment]) -> Value {
    json!(moments
        .iter()
        .map(|m| json!({
            "content": m.content,
            "created_at": m.created_at.format("%Y-%m-%d %H:%M").to_string(),
        }))
        .collect::<Vec<_>>())
}

/// 上一篇/下一篇 JSON；无则为 null。
fn adjacent_value(p: Option<&Post>) -> Value {
    match p {
        Some(p) => json!({ "slug": p.slug, "title": p.title }),
        None => json!(null),
    }
}

fn pagination_value(page: i64, total: i64) -> Value {
    pagination_with(page, total, PAGE_SIZE)
}

/// 说说页分页：与 `pagination_value` 同构，但按 MOMENTS_PAGE_SIZE（20）算总页数。
fn moments_pagination_value(page: i64, total: i64) -> Value {
    pagination_with(page, total, MOMENTS_PAGE_SIZE)
}

fn pagination_with(page: i64, total: i64, page_size: i64) -> Value {
    let total_pages = (total + page_size - 1) / page_size;
    json!({
        "current": page,
        "total": total,
        "total_pages": total_pages,
        "prev": (page > 1).then_some(page - 1),
        "next": (page < total_pages).then_some(page + 1),
    })
}

/// 说说列表上下文（首页最近说说 / 说说页共用）：`moments: Vec<{moment, attachments}>`，
/// 每条含附件（按 sort_order 升序）；模板渲染为时间线折叠样式。
async fn moment_items_value(db: &Db, base: &str, items: &[Moment]) -> AppResult<Value> {
    let mut out = Vec::with_capacity(items.len());
    for m in items {
        let atts = moments::list_moment_attachments(db, m.id).await?;
        out.push(json!({
            "moment": moment_value(m),
            "attachments": json!(
                atts.iter()
                    .map(|(att, _)| attachment_value(base, att))
                    .collect::<Vec<_>>()
            ),
        }));
    }
    Ok(json!(out))
}

/// 单条说说 JSON：id/内容/创建时间（RFC3339，模板 `| date` 过滤器展示本地时间）。
fn moment_value(m: &crate::models::Moment) -> Value {
    json!({
        "id": m.id,
        "content": m.content,
        "created_at": m.created_at.to_rfc3339(),
    })
}

/// 附件 JSON：kind 供模板分支（图片/视频/文件卡片），url 指向 /uploads 静态路径。
fn attachment_value(base: &str, att: &crate::models::Attachment) -> Value {
    json!({
        "id": att.id,
        "kind": att.kind.to_str(),
        "orig_name": att.orig_name,
        "mime": att.mime,
        "size": att.size,
        "url": format!("{base}/uploads/{}", att.path),
    })
}

/// 搜索页分页：与 `pagination_value` 同构，但 prev_url/next_url 预编码保留 `q`。
/// （tera 2.1.0 已移除 urlencode 过滤器，故在 Rust 侧完成编码。）
fn search_pagination_value(base: &str, page: i64, total: i64, q: &str) -> Value {
    let total_pages = (total + PAGE_SIZE - 1) / PAGE_SIZE;
    let q_enc = urlencode_q(q);
    json!({
        "current": page,
        "total": total,
        "total_pages": total_pages,
        "prev_url": (page > 1).then(|| format!("{base}/search?q={q_enc}&page={}", page - 1)),
        "next_url": (page < total_pages).then(|| format!("{base}/search?q={q_enc}&page={}", page + 1)),
    })
}

/// 查询串百分号编码（RFC 3986 保留字符，用于拼搜索分页链接）。
fn urlencode_q(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
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
/// `preview` 为预览主题名时，用该主题的 tera 临时渲染（每次请求构建：仅预览
/// 场景、流量极低；`AppState.tera` 启动时固定为默认主题无法覆盖），构建失败
/// 回退默认渲染，避免预览参数拖垮页面。
async fn render(state: &AppState, template: &str, ctx: &Context, preview: Option<&str>) -> Response {
    let preview_tera = match preview {
        Some(name) => match themes::build_tera(&state.config.data_dir.join("themes"), name) {
            Ok(t) => Some(t),
            Err(e) => {
                tracing::warn!("预览主题 {name} 模板加载失败，回退默认渲染: {e}");
                None
            }
        },
        None => None,
    };
    let tera = preview_tera.as_ref().unwrap_or(&state.tera);
    match tera.render(template, ctx) {
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
    let mut ctx = match site_context(&state.db, &state.config.base_path, None).await {
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

/// 错误页最后兜底：纯内联 HTML（message 先转义，M23：错误消息可能来自用户输入）。
fn fallback_error_page(status: StatusCode, message: &str) -> Response {
    let code = status.as_u16();
    let reason = status.canonical_reason().unwrap_or("");
    let safe_message = crate::util::html_escape(message);
    (
        status,
        Html(format!(
            "<!DOCTYPE html><html lang=\"zh-CN\"><meta charset=\"utf-8\">\
             <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
             <title>{code} {reason}</title>\
             <body style=\"font:17px/1.8 -apple-system,'PingFang SC',sans-serif;max-width:720px;margin:4rem auto;padding:0 1.25rem\">\
             <h1>{code} {reason}</h1><p>{safe_message}</p>\
             <p><a href=\"/\">返回首页</a></p></body></html>"
        )),
    )
        .into_response()
}
