//! 前台页面：文章流、文章/独立页、分类/标签归档、搜索、静态资源与错误页。
//!
//! 页面经 `site_context` 注入站点设置（settings 表）；`share` 分享元数据对象
//! 由文章/独立页 handler 单独注入，SSR 阶段在 `<head>` 输出 canonical、
//! Open Graph、Twitter Card。模板取自 `themes/<active_theme>/templates/`
//! （T6 `build_tera` 构建、注册 `markdown`/`date` 过滤器）。
//! 渲染失败统一输出 `error.html`（含状态码）。
//! 支持后台主题预览（T18）：`?theme_preview={name}` 只读覆盖
//! `site.active_theme` 与渲染用 tera（每次请求按预览主题构建），不落库。

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::models::{Moment, Post, PostStatus, PostType};
use crate::services::{likes, moments, posts, settings, stats, taxonomy};
use crate::{themes, AppState};
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Request, StatusCode, Uri, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fmt::Write as _;
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
/// 主题静态资源（main.js/style.css 等）：部署会更新，必须 no-cache，
/// 否则浏览器长缓存导致看不到新版（历史事故：复制按钮 toast 不弹）。
const THEME_CACHE: &str = "no-cache";

/// 前台路由（挂在 `/`），静态资源挂载点：
/// - `/uploads/{*path}` 附件目录
/// - `/theme/{name}/static/{*path}` 主题静态资源
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(index))
        .route("/archives", get(archives_page))
        .route("/post/{id}", get(post_page))
        .route("/page/{slug}", get(page_page))
        .route("/category/{slug}", get(category_page))
        .route("/tag/{slug}", get(tag_page))
        .route("/column/{slug}", get(column_page))
        .route("/columns", get(columns_page))
        .route("/about", get(about_page))
        .route("/moments", get(moments_page))
        .route("/trails", get(trails_page))
        .route("/trails/{id}", get(trail_page))
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
            "social_logos",
            "site_logo",
            "active_theme",
            "theme_mode",
            "footer_text",
            "friend_links",
            "contact_enabled",
            "contact_email",
            "date_format",
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
    let columns = crate::services::columns::list_columns(db).await?;
    // 导航归一化：每项 `{type, label, url}`。类型：home/首页、articles/文章、
    // moments/说说、pages/页面（下拉列出独立页）、column/专栏（下拉列出全部专栏）、
    // link/链接（自定义）。
    // 兼容旧数据：无 type 字段的名称为「文章」项 → articles；type=categories → pages
    // （旧「分类下拉」被「页面下拉」取代）。
    let nav_raw = parse_json_array(s.get("site_nav").map(String::as_str).unwrap_or("[]"));
    let nav: Vec<Value> = nav_raw
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let label = item.get("label").and_then(Value::as_str).unwrap_or("");
                    let url = item.get("url").and_then(Value::as_str).unwrap_or("");
                    let ty = item
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or("link");
                    let ty = if ty == "categories" {
                        "pages"
                    } else if item.get("type").is_none() && ty == "link" && label == "文章" {
                        "articles"
                    } else {
                        ty
                    };
                    // 后台「隐藏」开关：值为 true / "1" / 1 时前台导航不渲染；
                    // 首页与文章为站点基础入口，不允许隐藏（配置即使误设为隐藏也强制显示）
                    let hidden = json_hidden(item);
                    if hidden && ty != "home" && ty != "articles" {
                        return None;
                    }
                    // 轨迹类型路径预设（url 可空）
                    let url = if ty == "trail" && url.is_empty() {
                        "/trails"
                    } else {
                        url
                    };
                    Some(json!({ "type": ty, "label": label, "url": url }))
                })
                .collect()
        })
        .unwrap_or_default();
    // 独立页列表（type=page 已发布），供「页面」类型导航下拉
    let (pages, _pages_total) = posts::list_posts(
        db,
        posts::PostListOptions {
            status: Some(PostStatus::Published),
            post_type: Some(PostType::Page),
            category_slug: None,
            tag_slug: None,
            column_slug: None,
            month: None,
            sort: None,
            page: 1,
            page_size: 100,
        },
    )
    .await?;
    let pages_value: Vec<Value> = pages
        .iter()
        .map(|p| json!({ "slug": p.slug, "title": p.title }))
        .collect();
    // 社交条目数组化：有 logo 的进「二维码组」（social_qrs，hover 显示大图），
    // 无 logo 的进「链接组」（social_links，文字链接跳转）。顺序与后台拖拽排序一致。
    let social_raw = parse_json_array(s.get("site_social").map(String::as_str).unwrap_or("{}"));
    let social_logos = parse_json_array(s.get("social_logos").map(String::as_str).unwrap_or("{}"));
    let mut social_links: Vec<Value> = Vec::new();
    let mut social_qrs: Vec<Value> = Vec::new();
    if let Some(obj) = social_raw.as_object() {
        for (k, v) in obj {
            let logo = social_logos.get(k).and_then(Value::as_str).unwrap_or_default();
            let item = json!({
                "key": k,
                "url": v.as_str().unwrap_or_default(),
                "logo": logo,
            });
            if logo.is_empty() {
                social_links.push(item);
            } else {
                social_qrs.push(item);
            }
        }
    }
    // 仅配置了社交图标/二维码（未填对应链接）的平台也展示在二维码组
    if let Some(logos) = social_logos.as_object() {
        for (k, v) in logos {
            let exists = social_links
                .iter()
                .chain(social_qrs.iter())
                .any(|item| item["key"] == k.as_str());
            if !exists {
                social_qrs.push(json!({
                    "key": k,
                    "url": "",
                    "logo": v.as_str().unwrap_or_default(),
                }));
            }
        }
    }
    // 首页联动：导航把「说说/专栏」隐藏后，首页对应区块（最近说说 / 最热专栏）一并隐藏
    let moments_hidden = nav_has_hidden(&nav_raw, "moments");
    let columns_hidden = nav_has_hidden(&nav_raw, "column");

    ctx.insert(
        "site",
        &json!({
            "name": s.get("site_name").map(String::as_str).unwrap_or("寒蝉 Hancic"),
            "desc": s.get("site_desc").map(String::as_str).unwrap_or(""),
            "nav": nav,
            "show_moments": !moments_hidden,
            "show_columns": !columns_hidden,
            "pages": pages_value,
            "social_links": social_links,
            "social_qrs": social_qrs,
            "logo": s.get("site_logo").map(String::as_str).unwrap_or(""),
            "active_theme": active_theme,
            "mode": s.get("theme_mode").map(String::as_str).unwrap_or("auto"),
            "date_format": s.get("date_format").map(String::as_str).unwrap_or("datetime"),
            "footer_text": s.get("footer_text").map(String::as_str).unwrap_or(""),
            "friend_links": parse_json_array(s.get("friend_links").map(String::as_str).unwrap_or("[]")),
            "contact": json!({
                "enabled": s.get("contact_enabled").map(String::as_str).unwrap_or("0") == "1",
                "email": s.get("contact_email").map(String::as_str).unwrap_or(""),
            }),
            "categories": categories.iter().map(|c| json!({ "slug": c.slug, "name": c.name })).collect::<Vec<_>>(),
            "columns": columns.iter().map(|c| json!({ "slug": c.slug, "name": c.name })).collect::<Vec<_>>(),
        }),
    );
    Ok(ctx)
}

/// 解析 settings 中的 JSON 数组/对象字符串；非法时回退空值。

/// 导航项 hidden 判定：布尔 / "1" / 1 任一为真即隐藏。
fn json_hidden(item: &Value) -> bool {
    item.get("hidden")
        .map(|h| {
            h.as_bool().unwrap_or(false)
                || h.as_str().map(|s| s == "1").unwrap_or(false)
                || h.as_i64() == Some(1)
        })
        .unwrap_or(false)
}

/// 导航数组中是否存在指定类型且被隐藏的项（用于首页联动开关）。
fn nav_has_hidden(nav_raw: &Value, ty: &str) -> bool {
    nav_raw
        .as_array()
        .map(|arr| {
            arr.iter().any(|item| {
                item.get("type").and_then(Value::as_str) == Some(ty) && json_hidden(item)
            })
        })
        .unwrap_or(false)
}

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
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let preview = resolve_preview(&state, &query);
    let out = async {
        // 发布日历（近 26 周，含发布/说说详情）与最近说说（5 条）
        let calendar = posts::activity_calendar(&state.db, 182).await?;
        let (moments, _total) = moments::list_moments(&state.db, None, false, None, 1, 5).await?;
        // 最热专栏：文章数最多的专栏，最多取 3 个（按文章数降序）
        let hot_columns = {
            let cols = crate::services::columns::list_columns(&state.db).await?;
            let counts = crate::services::columns::count_columns_posts(&state.db).await?;
            let mut v: Vec<serde_json::Value> = cols
                .iter()
                .filter(|c| counts.get(&c.id).copied().unwrap_or(0) > 0)
                .map(|c| {
                    json!({
                        "slug": c.slug,
                        "name": c.name,
                        "description": c.description,
                        "count": counts.get(&c.id).copied().unwrap_or(0),
                    })
                })
                .collect();
            v.sort_by_key(|x| std::cmp::Reverse(x["count"].as_i64().unwrap_or(0)));
            v.truncate(3);
            v
        };
        // 最近文章（首页仅展示 5 篇，完整列表走 /archives）；支持 ?sort= 切换发布/更新时间
        let sort = list_sort(&query);
        let (items, total) = posts::list_posts(
            &state.db,
            posts::PostListOptions {
                status: Some(PostStatus::Published),
                post_type: Some(PostType::Post),
                category_slug: None,
                tag_slug: None,
                column_slug: None,
                month: None,
                sort: Some(sort),
                page: 1,
                page_size: 5,
            },
        )
        .await?;
        let mut ctx = site_context(&state.db, &state.config.base_path, preview.clone()).await?;
        ctx.insert("calendar", &calendar_matrix(&calendar));
        ctx.insert("hot_columns", &hot_columns);
        // 首页最近说说与说说页同构（moment + attachments），模板可渲染图片/视频附件
        ctx.insert(
            "moments",
            &moment_items_value(&state.db, &state.config, &headers, &state.config.base_path, &moments).await?,
        );
        ctx.insert("posts", &post_list_value(&state.db, &state.config.base_path, &items).await?);
        ctx.insert("post_total", &total);
        ctx.insert("current_sort", &sort.field);
        let sort_base = format!("{}/", state.config.base_path);
        ctx.insert("sort_base", &sort_base);
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
        let mut ctx = listing_ctx(&state.db, &state.config.base_path, page, None, None, None, month.clone(), list_sort(&query), preview.clone()).await?;
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
        ctx.insert("sort_base", &month_base);
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
    Path(id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let preview = resolve_preview(&state, &query);
    let out = async {
        let post = if let Some(post) = posts::get_post_by_uuid(&state.db, &id)
            .await?
            .filter(|p| p.status == PostStatus::Published && p.post_type == PostType::Post)
        {
            post
        } else if let Some(post) = posts::get_post_by_slug(&state.db, &id)
            .await?
            .filter(|p| p.status == PostStatus::Published && p.post_type == PostType::Post)
        {
            let target = format!("{}{}", state.config.base_path, posts::public_post_path(&post));
            return Ok::<_, AppError>(axum::response::Redirect::permanent(&target).into_response());
        } else {
            return Err(AppError::NotFound("文章不存在".into()));
        };
        let (prev, next) = posts::adjacent_posts(&state.db, &post).await?;
        let tags = posts::list_tags_of_post(&state.db, post.id).await?;
        let category = category_of(&state.db, post.category_id).await?;
        let column = column_of(&state.db, post.column_id).await?;
        let like_status = like_status_from_headers(
            &state.db,
            &state.config,
            &headers,
            crate::models::LikeContentType::Post,
            post.id,
            post.like_count,
        )
        .await?;
        let mut ctx = site_context(&state.db, &state.config.base_path, preview.clone()).await?;
        ctx.insert("post", &post_value(&post, like_status.liked));
        let site = ctx.get("site");
        let share = share_context(
            &post.title,
            post.excerpt.as_str(),
            &post.content_md,
            &format!("/post/{}", post.uuid),
            site.and_then(|v| v.get_from_path("name")).and_then(|v| v.as_str()).unwrap_or("寒蝉 Hancic"),
            &state.config.site_url,
            site.and_then(|v| v.get_from_path("logo")).and_then(|v| v.as_str()).filter(|l| !l.trim().is_empty()),
        );
        ctx.insert("share", &share);
        ctx.insert(
            "category",
            &category
                .map(|c| json!({ "slug": c.slug, "name": c.name }))
                .unwrap_or(json!(null)),
        );
        ctx.insert(
            "column",
            &column
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
        let (content_html, toc) = crate::markdown::render_with_toc(&post.content_md);
        ctx.insert("content_html", &content_html);
        ctx.insert(
            "toc",
            &json!(toc
                .iter()
                .map(|t| json!({ "level": t.level, "text": t.text, "id": format!("toc-{}", t.id) }))
                .collect::<Vec<_>>()),
        );
        let word_count = html_word_count(&content_html);
        ctx.insert("word_count", &word_count);
        ctx.insert("read_minutes", &word_count.div_ceil(300).max(1));
        record_view_once(&state, &post, &headers).await;
        Ok::<_, AppError>(render(&state, "post.html", &ctx, preview.as_deref()).await)
    }
    .await;
    match out {
        Ok(resp) => resp,
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
        ctx.insert("page", &post_value(&page, false));
        let site = ctx.get("site");
        let share = share_context(
            &page.title,
            page.excerpt.as_str(),
            &page.content_md,
            &format!("/page/{}", page.slug),
            site.and_then(|v| v.get_from_path("name")).and_then(|v| v.as_str()).unwrap_or("寒蝉 Hancic"),
            &state.config.site_url,
            site.and_then(|v| v.get_from_path("logo")).and_then(|v| v.as_str()).filter(|l| !l.trim().is_empty()),
        );
        ctx.insert("share", &share);
        // 页面正文（带标题锚点）+ 目录，供右侧栏导航（内容长时便于跳转）
        let (content_html, toc) = crate::markdown::render_with_toc(&page.content_md);
        ctx.insert("content_html", &content_html);
        ctx.insert(
            "toc",
            &json!(toc
                .iter()
                .map(|t| json!({ "level": t.level, "text": t.text, "id": format!("toc-{}", t.id) }))
                .collect::<Vec<_>>()),
        );
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
        ctx.insert("page", &post_value(&page, false));
        let site = ctx.get("site");
        let share = share_context(
            &page.title,
            page.excerpt.as_str(),
            &page.content_md,
            "/about",
            site.and_then(|v| v.get_from_path("name")).and_then(|v| v.as_str()).unwrap_or("寒蝉 Hancic"),
            &state.config.site_url,
            site.and_then(|v| v.get_from_path("logo")).and_then(|v| v.as_str()).filter(|l| !l.trim().is_empty()),
        );
        ctx.insert("share", &share);
        let (content_html, toc) = crate::markdown::render_with_toc(&page.content_md);
        ctx.insert("content_html", &content_html);
        ctx.insert(
            "toc",
            &json!(toc
                .iter()
                .map(|t| json!({ "level": t.level, "text": t.text, "id": format!("toc-{}", t.id) }))
                .collect::<Vec<_>>()),
        );
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
    headers: HeaderMap,
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
        ctx.insert(
            "moments",
            &moment_items_value(&state.db, &state.config, &headers, &state.config.base_path, &items).await?,
        );
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
            listing_ctx(&state.db, &state.config.base_path, page_param(&query), Some(slug), None, None, None, list_sort(&query), preview.clone()).await?;
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
        let sort_base = format!("{}/category/{}", state.config.base_path, category.slug);
        ctx.insert("sort_base", &sort_base);
        Ok::<_, AppError>(ctx)
    }
    .await;
    match out {
        Ok(ctx) => render(&state, "category.html", &ctx, preview.as_deref()).await,
        Err(e) => render_error(&state, e).await,
    }
}

/// 专栏归档页：该专栏下文章分页列表（复用 listing_ctx，含排序条）。
async fn column_page(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let preview = resolve_preview(&state, &query);
    let out = async {
        let column = crate::services::columns::get_column_by_slug(&state.db, &slug)
            .await?
            .ok_or_else(|| AppError::NotFound("专栏不存在".into()))?;
        // 专栏页默认按发布时间倒序（最新在前）；显式 ?sort= 才走通用排序
        let sort = if query.contains_key("sort") {
            list_sort(&query)
        } else {
            posts::PostSort { field: "published_at", asc: false }
        };
        let mut ctx = listing_ctx(
            &state.db,
            &state.config.base_path,
            page_param(&query),
            None,
            None,
            Some(slug),
            None,
            sort,
            preview.clone(),
        )
        .await?;
        ctx.insert(
            "column",
            &json!({ "slug": column.slug, "name": column.name }),
        );
        let sort_base = format!("{}/column/{}", state.config.base_path, column.slug);
        ctx.insert("sort_base", &sort_base);
        Ok::<_, AppError>(ctx)
    }
    .await;
    match out {
        Ok(ctx) => render(&state, "column.html", &ctx, preview.as_deref()).await,
        Err(e) => render_error(&state, e).await,
    }
}

/// 专栏卡片总览页：每个专栏一张卡片（名称+描述+该专栏文章链接列表）。
const COLUMN_CARD_POSTS: i64 = 10;

async fn columns_page(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let preview = resolve_preview(&state, &query);
    let base = state.config.base_path.clone();
    let out = async {
        let mut ctx = site_context(&state.db, &base, preview.clone()).await?;
        let columns = crate::services::columns::list_columns(&state.db).await?;
        let mut cards = Vec::with_capacity(columns.len());
        for c in &columns {
            let (items, total) = posts::list_posts(
                &state.db,
                posts::PostListOptions {
                    status: Some(PostStatus::Published),
                    post_type: Some(PostType::Post),
                    category_slug: None,
                    tag_slug: None,
                    column_slug: Some(c.slug.clone()),
                    month: None,
                    sort: Some(posts::PostSort { field: "column_sort", asc: true }),
                    page: 1,
                    page_size: COLUMN_CARD_POSTS,
                },
            )
            .await?;
            cards.push(json!({
                "slug": c.slug,
                "name": c.name,
                "description": c.description,
                "total": total,
                "posts": items.iter().map(|p| json!({
                    "title": p.title,
                    "url": post_url(&base, p),
                    "published_at": p.published_at.map(|d| d.to_rfc3339()),
                })).collect::<Vec<_>>(),
            }));
        }
        ctx.insert("columns", &json!(cards));
        ctx.insert("has_columns", &!columns.is_empty());
        Ok::<_, AppError>(ctx)
    }
    .await;
    match out {
        Ok(ctx) => render(&state, "column_list.html", &ctx, preview.as_deref()).await,
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
            None,
            month.clone(),
            list_sort(&query),
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
        ctx.insert("sort_base", &month_base);
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
        // 全局搜索：同时匹配说说（最多 20 条，按时间倒序）
        let moments = if q.trim().is_empty() {
            Vec::new()
        } else {
            moments::search_moments(&state.db, q.trim(), 20).await?
        };
        ctx.insert("moments", &moment_search_value(&moments));
        Ok::<_, AppError>(ctx)
    }
    .await;
    match out {
        Ok(ctx) => render(&state, "search.html", &ctx, preview.as_deref()).await,
        Err(e) => render_error(&state, e).await,
    }
}

// ---------- 徒步轨迹 ----------

/// 轨迹总览颜色盘（不同轨迹用不同颜色区分；按列表序号取模）。
const TRAIL_PALETTE: &[&str] = &[
    "#d4442a", "#2a7dd4", "#1a9d4f", "#8e44ad", "#d49a2a", "#1aad9e", "#d4448a", "#5a63d4",
];

/// 轨迹总览：Leaflet 地图（所有轨迹简化线，可切换瓦片源）+ 轨迹卡片列表。
async fn trails_page(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let preview = resolve_preview(&state, &query);
    let out = async {
        let trails = crate::services::trails::list_trails(
            &state.db,
            crate::services::trails::TrailSort::Recent,
        )
        .await?;
        let mut ctx = site_context(&state.db, &state.config.base_path, preview.clone()).await?;
        // 地图数据：id/名称/颜色/简化坐标（safe_string 内嵌，避免 tera 转义 JSON）
        let trails_dir = state.config.data_dir.join("trails");
        let map_trails: Vec<Value> = trails
            .iter()
            .enumerate()
            .map(|(i, t)| {
                // 完整坐标优先（不抽稀，轨迹线更精细）；缺失时回退抽稀 simplified
                let coords = crate::services::trails::load_full_coords(&trails_dir, t.id)
                    .map(|c| json!(c))
                    .unwrap_or_else(|| {
                        serde_json::from_str::<Value>(&t.simplified).unwrap_or_else(|_| json!([]))
                    });
                json!({
                    "id": t.id,
                    "name": t.name,
                    "color": TRAIL_PALETTE[i % TRAIL_PALETTE.len()],
                    "coords": coords,
                    "description": t.description,
                    "distance_km_str": t.distance_m.map(|m| format!("{:.1}", m / 1000.0)),
                    "elevation_gain_str": t.elevation_gain_m.map(|e| format!("{e:.0}")),
                    "elevation_loss_str": t.elevation_loss_m.map(|e| format!("{e:.0}")),
                    "moving": crate::services::trails::format_moving(t.moving_seconds),
                    "avg_speed_str": t.avg_speed_kmh.map(|s| format!("{s:.1}")),
                    "max_ele_str": t.max_elevation_m.map(|e| format!("{e:.0}")),
                    "min_ele_str": t.min_elevation_m.map(|e| format!("{e:.0}")),
                    "point_count": t.point_count,
                    "started_at": t.started_at.map(|d| d.to_rfc3339()),
                    "distance_m": t.distance_m,
                    "elevation_gain_m": t.elevation_gain_m,
                    "moving_seconds": t.moving_seconds,
                    "url": format!("/trails/{}", t.id),
                })
            })
            .collect();
        ctx.insert_value(
            "trails_json",
            tera::Value::safe_string(&json!(map_trails).to_string()),
        );
        ctx.insert("trails", &trail_card_value(&trails));
        ctx.insert("trail_count", &trails.len());
        ctx.insert("has_trails", &!trails.is_empty());
        Ok::<_, AppError>(ctx)
    }
    .await;
    match out {
        Ok(ctx) => render(&state, "trails.html", &ctx, preview.as_deref()).await,
        Err(e) => render_error(&state, e).await,
    }
}

/// 轨迹详情：完整轨迹地图 + 数据卡片 + 描述；不存在 → 404。
async fn trail_page(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    // 非数字 id（如 /trails/abc）按 404 处理，与"轨迹不存在"一致
    let id = match id.parse::<i64>() {
        Ok(id) => id,
        Err(_) => return render_error(&state, AppError::NotFound("轨迹不存在".into())).await,
    };
    let preview = resolve_preview(&state, &query);
    let out = async {
        let trail = crate::services::trails::get_trail(&state.db, id)
            .await?
            .ok_or_else(|| AppError::NotFound("轨迹不存在".into()))?;
        let trails_dir = state.config.data_dir.join("trails");
        // 完整坐标：优先导入时落盘的 {id}.json；缺失时解析 GPX 兜底
        let coords = crate::services::trails::load_full_coords(&trails_dir, id).or_else(|| {
            tracing::warn!("轨迹完整坐标缺失 id={id}，回退解析 GPX 原文件");
            crate::services::trails::load_gpx_coords(&trails_dir, &trail.file_path)
        });
        let coords = coords.ok_or_else(|| AppError::Internal("轨迹坐标缺失".into()))?;
        let mut ctx = site_context(&state.db, &state.config.base_path, preview.clone()).await?;
        ctx.insert_value(
            "trail_coords_json",
            tera::Value::safe_string(&crate::services::trails::coords_json(&coords)),
        );
        ctx.insert("trail", &trail_detail_value(&trail));
        Ok::<_, AppError>(ctx)
    }
    .await;
    match out {
        Ok(ctx) => render(&state, "trail.html", &ctx, preview.as_deref()).await,
        Err(e) => render_error(&state, e).await,
    }
}

/// 总览卡片 JSON：名称/日期/里程/爬升/点数/详情链接（显示字符串已格式化）。
fn trail_card_value(trails: &[crate::models::Trail]) -> Value {
    json!(trails
        .iter()
        .map(|t| json!({
            "id": t.id,
            "name": t.name,
            "description": t.description,
            "distance_km": t.distance_m.map(|m| m / 1000.0),
            "distance_km_str": t.distance_m.map(|m| format!("{:.1}", m / 1000.0)),
            "elevation_gain_m": t.elevation_gain_m,
            "elevation_gain_str": t.elevation_gain_m.map(|e| format!("{e:.0}")),
            "point_count": t.point_count,
            "started_at": t.started_at.map(|d| d.to_rfc3339()),
            "moving": crate::services::trails::format_moving(t.moving_seconds),
            "url": format!("/trails/{}", t.id),
        }))
        .collect::<Vec<_>>())
}

/// 详情 JSON：数据卡片全部原始数值 + 展示用格式化字段。
fn trail_detail_value(t: &crate::models::Trail) -> Value {
    json!({
        "id": t.id,
        "name": t.name,
        "description": t.description,
        "distance_km": t.distance_m.map(|m| m / 1000.0),
        "distance_km_str": t.distance_m.map(|m| format!("{:.1}", m / 1000.0)),
        "elevation_gain_m": t.elevation_gain_m,
        "elevation_gain_str": t.elevation_gain_m.map(|e| format!("{e:.0}")),
        "elevation_loss_m": t.elevation_loss_m,
        "elevation_loss_str": t.elevation_loss_m.map(|e| format!("{e:.0}")),
        "moving": crate::services::trails::format_moving(t.moving_seconds),
        "avg_speed_kmh": t.avg_speed_kmh,
        "avg_speed_str": t.avg_speed_kmh.map(|s| format!("{s:.1}")),
        "max_elevation_m": t.max_elevation_m,
        "max_elevation_str": t.max_elevation_m.map(|e| format!("{e:.0}")),
        "min_elevation_m": t.min_elevation_m,
        "min_elevation_str": t.min_elevation_m.map(|e| format!("{e:.0}")),
        "started_at": t.started_at.map(|d| d.to_rfc3339()),
        "point_count": t.point_count,
        "start": if t.start_lat.is_some() && t.start_lon.is_some() {
            json!([t.start_lat.unwrap(), t.start_lon.unwrap()])
        } else {
            json!(null)
        },
        "end": if t.end_lat.is_some() && t.end_lon.is_some() {
            json!([t.end_lat.unwrap(), t.end_lon.unwrap()])
        } else {
            json!(null)
        },
    })
}
/// 未匹配路由 → 404 错误页。
pub async fn not_found(State(state): State<AppState>) -> Response {
    render_error(&state, AppError::NotFound("页面不存在".into())).await
}

// ---------- 静态资源 ----------

async fn serve_uploads(State(state): State<AppState>, req: Request<Body>) -> Response {
    serve_from(state.config.data_dir.join("uploads"), "/uploads", req, STATIC_CACHE).await
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
    serve_from(base, &format!("/theme/{name}/static"), req, THEME_CACHE).await
}

/// 去掉挂载前缀后用 ServeDir 服务目录，并附加缓存头。
async fn serve_from(base: PathBuf, prefix: &str, req: Request<Body>, cache: &'static str) -> Response {
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
                    header::HeaderValue::from_static(cache),
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
/// 文章流列表上下文（分类/标签/专栏/归档共用）：注入 posts + 排序切换 + pagination。
#[allow(clippy::too_many_arguments)]
async fn listing_ctx(
    db: &Db,
    base: &str,
    page: i64,
    category_slug: Option<String>,
    tag_slug: Option<String>,
    column_slug: Option<String>,
    month: Option<String>,
    sort: posts::PostSort,
    preview: Option<String>,
) -> AppResult<Context> {
    let (items, total) = posts::list_posts(
        db,
        posts::PostListOptions {
            status: Some(PostStatus::Published),
            post_type: Some(PostType::Post),
            category_slug,
            tag_slug,
            column_slug,
            month,
            sort: Some(sort),
            page,
            page_size: PAGE_SIZE,
        },
    )
    .await?;
    let mut ctx = site_context(db, base, preview).await?;
    ctx.insert("posts", &post_list_value(db, base, &items).await?);
    ctx.insert("pagination", &pagination_value(page, total));
    // 列表页排序切换（post_list.html 条件渲染；index 不注入）
    ctx.insert("sort_ctl", &true);
    ctx.insert("current_sort", &sort.field);
    ctx.insert("sort_base", base);
    // 列表页文章卡片展示点赞数（归档/分类/标签/专栏）
    ctx.insert("show_like", &true);
    // 列表总数（专栏页标题等处展示）
    ctx.insert("post_total", &total);
    Ok(ctx)
}

/// 前台列表排序：`?sort=updated_at|published_at|views|like_count`，默认按发布时间倒序。
fn list_sort(query: &HashMap<String, String>) -> posts::PostSort {
    let field = match query.get("sort").map(String::as_str) {
        Some("updated_at") => "updated_at",
        Some("views") => "views",
        Some("like_count") => "like_count",
        _ => "published_at",
    };
    posts::PostSort { field, asc: false }
}

/// 列表页文章 JSON：标题/日期/excerpt/阅读量/字数/预计阅读时长/链接。
async fn post_list_value(db: &Db, base: &str, items: &[Post]) -> AppResult<Value> {
    let mut list = Vec::with_capacity(items.len());
    for p in items {
        let tags = posts::list_tags_of_post(db, p.id).await?;
        let (html, _) = crate::markdown::render_with_toc(&p.content_md);
        let word_count = html_word_count(&html);
        list.push(json!({
            "id": p.id,
            "slug": p.slug,
            "title": p.title,
            "excerpt": p.excerpt,
            "published_at": p.published_at.map(|d| d.to_rfc3339()),
            "updated_at": p.updated_at.to_rfc3339(),
            "views": p.views,
            "like_count": p.like_count,
            "word_count": word_count,
            "read_minutes": word_count.div_ceil(300).max(1),
            "url": post_url(base, p),
            "tags": tags.iter().map(|t| json!({ "slug": t.slug, "name": t.name })).collect::<Vec<_>>(),
        }));
    }
    Ok(json!(list))
}

/// 根据请求 cookie 返回当前访客对内容的点赞状态；无 visitor cookie 时视为未点赞。
async fn like_status_from_headers(
    db: &Db,
    config: &std::sync::Arc<crate::config::Config>,
    headers: &HeaderMap,
    content_type: crate::models::LikeContentType,
    content_id: i64,
    current_like_count: i64,
) -> AppResult<likes::LikeStatus> {
    match crate::api::likes::ensure_visitor_cookie(config, headers) {
        (visitor_id, None) => likes::like_status(db, content_type, content_id, &visitor_id).await,
        (_visitor_id, Some(_)) => Ok(likes::LikeStatus {
            liked: false,
            like_count: current_like_count,
        }),
    }
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

/// 搜索页说说结果 JSON：`{ snippet, date }`。摘要去掉常见 markdown 标记后截断。
fn moment_search_value(moments: &[(i64, String, String)]) -> Value {
    fn clean(md: &str) -> String {
        let mut s = md
            .replace("```", "")
            .replace("**", "")
            .replace("__", "")
            .replace("`", "");
        // 行内链接 [text](url) → 整段移除（保留其余文字）
        loop {
            let Some(close) = s.find("](") else { break };
            let Some(open) = s.rfind('[').filter(|&i| i < close) else { break };
            let end = s[close + 2..].find(')').map(|e| close + 2 + e);
            match end {
                Some(end) => {
                    s.replace_range(open..=end, "");
                }
                None => {
                    s.replace_range(open..close + 2, "");
                }
            }
        }
        // 去掉行首 markdown 标记（标题 # / 列表 - / 引用 >）
        let s: String = s
            .lines()
            .map(|l| {
                let t = l.trim_start();
                let t = t
                    .trim_start_matches('#')
                    .trim_start_matches('-')
                    .trim_start_matches('>');
                t.trim_start()
            })
            .collect::<Vec<_>>()
            .join(" ");
        let s = s.trim();
        if s.chars().count() > 100 {
            s.chars().take(100).collect::<String>() + "…"
        } else {
            s.to_string()
        }
    }
    json!(moments
        .iter()
        .map(|(_, content, date)| json!({
            "snippet": clean(content),
            "date": date,
        }))
        .collect::<Vec<_>>())
}

/// 文章页上下文 JSON。
fn post_value(p: &Post, liked: bool) -> Value {
    json!({
        "id": p.id,
        "slug": p.slug,
        "title": p.title,
        "content_md": p.content_md,
        "excerpt": p.excerpt,
        "published_at": p.published_at.map(|d| d.to_rfc3339()),
        "updated_at": p.updated_at.to_rfc3339(),
        "views": p.views,
        "like_count": p.like_count,
        "liked": liked,
    })
}

pub fn normalized_site_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

pub fn absolute_url(site_url: Option<&str>, path: &str) -> Option<String> {
    let origin = site_url?.trim_end_matches('/');
    Some(format!("{origin}{path}"))
}

/// 剥离 HTML 标签并解码常见 HTML 实体的纯文本摘要（≤ max_chars 字符）。
/// 实体解码覆盖分享摘要常见的命名实体（`&amp;` `&lt;` `&gt;` `&quot;`
/// `&apos;` `&nbsp;` 及常用排版实体）与数字引用（`&#NN;` / `&#xHH;`）；
/// 未知实体与裸 `&` 原样保留。最小实现，不引入外部依赖。
fn plain_text_summary(input: &str, max_chars: usize) -> String {
    let html = crate::markdown::render(input);
    let chars: Vec<char> = html.chars().collect();
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut last_was_space = true;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            '&' if !in_tag => {
                match decode_entity_at(&chars, i) {
                    Some((decoded, consumed)) => {
                        // 解码出的空白（如 &nbsp;）同样折叠成普通空格
                        if decoded.is_whitespace() {
                            if !last_was_space {
                                text.push(' ');
                                last_was_space = true;
                            }
                        } else {
                            text.push(decoded);
                            last_was_space = false;
                        }
                        i += consumed;
                        continue;
                    }
                    None => {
                        text.push('&');
                        last_was_space = false;
                    }
                }
            }
            c if in_tag => {
                let _ = c;
            }
            c if c.is_whitespace() => {
                if !last_was_space {
                    text.push(' ');
                    last_was_space = true;
                }
            }
            c => {
                text.push(c);
                last_was_space = false;
            }
        }
        i += 1;
    }
    let mut out = String::new();
    for ch in text.trim().chars().take(max_chars) {
        let _ = out.write_char(ch);
    }
    out.trim().to_string()
}

/// 在 `chars[i] == '&'` 处解析一个 HTML 实体：命名（`&amp;` 等）或数字
/// （`&#123;` / `&#x1f;`）。成功返回（解码字符，含 `&` 在内的消耗字符数）。
fn decode_entity_at(chars: &[char], i: usize) -> Option<(char, usize)> {
    let mut j = i + 1;
    // 数字引用：&#123; / &#x1f;
    if chars.get(j) == Some(&'#') {
        let mut j = i + 2;
        let mut radix = 10u32;
        if matches!(chars.get(j), Some('x') | Some('X')) {
            radix = 16;
            j += 1;
        }
        let digits_start = j;
        let mut val = 0u32;
        while let Some(&c) = chars.get(j) {
            if c == ';' {
                break;
            }
            if j - digits_start >= 8 {
                return None;
            }
            val = val.checked_mul(radix)?.checked_add(c.to_digit(radix)?)?;
            j += 1;
        }
        if j == digits_start || chars.get(j) != Some(&';') {
            return None;
        }
        return Some((char::from_u32(val)?, j + 1 - i));
    }
    // 命名引用：&amp; 等（≤16 字母数字）
    let name_start = j;
    while let Some(&c) = chars.get(j) {
        if c == ';' {
            break;
        }
        if !c.is_ascii_alphanumeric() || j - name_start >= 16 {
            return None;
        }
        j += 1;
    }
    if j == name_start || chars.get(j) != Some(&';') {
        return None;
    }
    let name: String = chars[name_start..j].iter().collect();
    let decoded = match name.as_str() {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{00A0}',
        "hellip" => '\u{2026}',
        "mdash" => '\u{2014}',
        "ndash" => '\u{2013}',
        "middot" => '\u{00B7}',
        "copy" => '\u{00A9}',
        "lsquo" => '\u{2018}',
        "rsquo" => '\u{2019}',
        "ldquo" => '\u{201C}',
        "rdquo" => '\u{201D}',
        _ => return None,
    };
    Some((decoded, j + 1 - i))
}

pub fn share_summary(excerpt: &str, content_md: &str) -> String {
    let excerpt = excerpt.trim();
    if !excerpt.is_empty() {
        return excerpt.to_string();
    }
    plain_text_summary(content_md, 160)
}

pub fn share_context(
    title: &str,
    excerpt: &str,
    content_md: &str,
    canonical_path: &str,
    site_name: &str,
    site_url: &str,
    logo_path: Option<&str>,
) -> Value {
    let normalized_site = normalized_site_url(site_url);
    let canonical_url = absolute_url(normalized_site.as_deref(), canonical_path);
    let og_image = logo_path
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .and_then(|path| absolute_url(normalized_site.as_deref(), path));
    json!({
        "title": title,
        "description": share_summary(excerpt, content_md),
        "site_name": site_name,
        "canonical_url": canonical_url,
        "og_url": canonical_url,
        "og_image": og_image,
        "twitter_card": if og_image.is_some() { "summary_large_image" } else { "summary" },
    })
}

/// 文章链接：独立页走 `/page/`，普通文章走 `/post/{uuid}`；`base` 为部署子路径前缀。
fn post_url(base: &str, p: &Post) -> String {
    let path = if p.post_type == PostType::Page {
        format!("/page/{}", p.slug)
    } else {
        format!("/post/{}", p.uuid)
    };
    format!("{base}{path}")
}

/// 发布日历单日条目：(日期, 动态数, [(类型, 标题)])。
type CalendarDay = (String, i64, Vec<(String, String)>);
/// 日历查询表键/值别名。
type DayKey<'a> = &'a str;
type DayVal<'a> = (i64, &'a [(String, String)]);

/// GitHub 风格发布日历矩阵：26 周 × 7 天。
/// 外层数组 = 周（列），内层 = 该周 7 天（行，周日→周六）。
/// 每格：`{ date, count, level, tip }`，tip 为多行 tooltip（日期 + 发布动态标题）。
fn calendar_matrix(calendar: &[CalendarDay]) -> Value {
    use std::collections::HashMap;
    let map: HashMap<DayKey<'_>, DayVal<'_>> = calendar
        .iter()
        .map(|(d, c, items)| (d.as_str(), (*c, items.as_slice())))
        .collect();
    let today = chrono::Utc::now().date_naive();
    let start = today - chrono::Days::new(181);
    let mut weeks: Vec<Vec<Value>> = Vec::new();
    let mut week: Vec<Value> = Vec::new();
    for i in 0..182 {
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

/// 上一篇/下一篇 JSON；无则为 null。
fn adjacent_value(p: Option<&Post>) -> Value {
    match p {
        Some(p) => json!({ "url": post_url("", p), "title": p.title }),
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
async fn moment_items_value(
    db: &Db,
    config: &std::sync::Arc<crate::config::Config>,
    headers: &HeaderMap,
    base: &str,
    items: &[Moment],
) -> AppResult<Value> {
    let mut out = Vec::with_capacity(items.len());
    for m in items {
        let atts = moments::list_moment_attachments(db, m.id).await?;
        let like_status = like_status_from_headers(
            db,
            config,
            headers,
            crate::models::LikeContentType::Moment,
            m.id,
            m.like_count,
        )
        .await?;
        out.push(json!({
            "moment": moment_value(m, like_status.liked),
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
fn moment_value(m: &crate::models::Moment, liked: bool) -> Value {
    json!({
        "id": m.id,
        "content": m.content,
        "created_at": m.created_at.to_rfc3339(),
        "like_count": m.like_count,
        "liked": liked,
    })
}

/// 附件 JSON：kind 供模板分支（图片/视频/文件卡片），url 指向 /uploads 静态路径。
fn attachment_value(base: &str, att: &crate::models::Attachment) -> Value {
    json!({
        "id": att.id,
        "kind": att.kind.to_str(),
        "orig_name": crate::util::percent_decode(&att.orig_name),
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

/// 统计渲染后 HTML 的非空白字符数（去标签），用于阅读字数/时长估算。
fn html_word_count(html: &str) -> usize {
    let mut count = 0usize;
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag && !c.is_whitespace() => count += 1,
            _ => {}
        }
    }
    count
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

/// 文章所属专栏（按 id 查找）。
async fn column_of(db: &Db, id: Option<i64>) -> AppResult<Option<crate::models::Column>> {
    match id {
        Some(id) => Ok(crate::services::columns::list_columns(db)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_summary_decodes_named_entities_and_strips_tags() {
        // 标签剥离 + 命名/数字实体解码 + 未知实体原样保留
        let summary = plain_text_summary("A <b>B</b> &amp; C &lt;D&gt; &#43; &unknown;", 200);
        assert_eq!(summary, "A B & C <D> + &unknown;");
    }

    #[test]
    fn decode_entity_at_handles_named_numeric_and_unknown() {
        let s: Vec<char> = "&amp; &lt; &#43; &#x2B; &unknown;".chars().collect();
        assert_eq!(decode_entity_at(&s, 0), Some(('&', 5)));
        assert_eq!(decode_entity_at(&s, 6), Some(('<', 4)));
        assert_eq!(decode_entity_at(&s, 11), Some(('+', 5)));
        assert_eq!(decode_entity_at(&s, 17), Some(('+', 6)));
        assert_eq!(decode_entity_at(&s, 24), None);
    }
}
