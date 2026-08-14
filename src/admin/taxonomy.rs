//! 后台分类与标签管理：两栏页（分类 + 标签），分类增改删、标签增删。
//!
//! 鉴权约定同其他后台模块：GET 未登录 302 跳登录；POST 先 `require_admin`
//! 再过 CSRF。操作结果统一 302 回 `/admin/taxonomy`，失败带 `?msg=` 错误回显
//! （空名称、slug 冲突等）。删除分类依赖 `ON DELETE SET NULL` 保留关联文章，
//! 删除标签依赖 `post_tags` 级联清空关联（CASCADE）。
//!
//! UI 只展示分类名称（slug 由名称自动生成；改名保留原 slug 以免破坏前台
//! 分类页链接；无排序概念，sort_order 仅建库默认 0）。

use crate::error::AppError;
use crate::models::{Category, Tag};
use crate::services::{posts, taxonomy};
use crate::{session, AppState};
use axum::extract::{Form, OriginalUri, Path, Query, State};
use axum::response::Response;
use serde_json::{Value, json};
use std::collections::HashMap;
use tower_sessions::Session;

// ---------- 列表 ----------

pub async fn list(
    State(state): State<AppState>,
    session: Session,
    Query(query): Query<HashMap<String, String>>,
    uri: OriginalUri,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect(&state.config.base_path,  "/admin/login");
    }
    let categories = taxonomy::list_categories(&state.db).await.unwrap_or_default();
    let tags = taxonomy::list_tags(&state.db).await.unwrap_or_default();
    let cat_counts = taxonomy::count_categories_posts(&state.db).await.unwrap_or_default();
    let tag_counts = taxonomy::count_tags_posts(&state.db).await.unwrap_or_default();
    let (mut ctx, _csrf) = super::base_ctx(&state, &session, uri.path()).await;
    ctx.insert("categories", &categories_value(&categories, &cat_counts));
    ctx.insert("tags", &tags_value(&tags, &tag_counts));
    // POST 失败回显的错误提示（`?msg=`，见 `fail`）
    ctx.insert(
        "error_msg",
        &query.get("msg").map(String::as_str).unwrap_or(""),
    );
    super::render_admin(&state, "taxonomy.html", &ctx)
}

// ---------- 新建分类 ----------

pub async fn create_category(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<HashMap<String, String>>,
) -> Result<Response, AppError> {
    session::require_admin(&session).await?;
    session::verify_csrf(&session, form.get("csrf").map(String::as_str)).await?;
    let name = form.get("name").cloned().unwrap_or_default();
    if name.trim().is_empty() {
        return Ok(fail(&state.config.base_path, "分类名称不能为空"));
    }
    if name.trim().chars().count() > 5 {
        return Ok(fail(&state.config.base_path, "分类名称最多 5 个字"));
    }
    let slug = parse_slug(form.get("slug"), &name).await;
    let sort_order = parse_sort_order(form.get("sort_order"));
    match taxonomy::create_category(&state.db, &name, &slug, sort_order).await {
        Ok(_) => Ok(super::redirect(&state.config.base_path,  "/admin/taxonomy")),
        // slug 唯一约束冲突由服务层返回 Conflict，作为错误回显而非 409 落页
        Err(AppError::Conflict(_)) => Ok(fail(&state.config.base_path, "分类 slug 已存在")),
        Err(e) => {
            tracing::error!("创建分类失败: {e:?}");
            Ok(fail(&state.config.base_path, "创建分类失败，请重试"))
        }
    }
}

// ---------- 更新分类 ----------

pub async fn update_category(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<i64>,
    Form(form): Form<HashMap<String, String>>,
) -> Result<Response, AppError> {
    session::require_admin(&session).await?;
    session::verify_csrf(&session, form.get("csrf").map(String::as_str)).await?;
    let name = form.get("name").cloned().unwrap_or_default();
    if name.trim().is_empty() {
        return Ok(fail(&state.config.base_path, "分类名称不能为空"));
    }
    if name.trim().chars().count() > 5 {
        return Ok(fail(&state.config.base_path, "分类名称最多 5 个字"));
    }
    // 表单不再提供 slug/sort_order（UI 只留名称）：保留库中原值，
    // 避免改名导致前台分类页链接失效。
    let existing = taxonomy::get_category_by_id(&state.db, id).await?;
    let slug = match form.get("slug").map(String::as_str).unwrap_or("").trim() {
        "" => existing.as_ref().map(|c| c.slug.clone()).unwrap_or_default(),
        s => s.to_string(),
    };
    // slug 冲突预检：update_category 直接写库不查重，撞 UNIQUE 约束会 500，
    // 提前比对其他分类，命中则错误回显（与新建路径同一提示）。
    if let Some(other) = taxonomy::get_category_by_slug(&state.db, &slug).await? {
        if other.id != id {
            return Ok(fail(&state.config.base_path, "分类 slug 已存在"));
        }
    }
    let sort_order = match form.get("sort_order") {
        Some(v) => v.trim().parse::<i64>().unwrap_or(0),
        None => existing.map(|c| c.sort_order).unwrap_or(0),
    };
    match taxonomy::update_category(&state.db, id, &name, &slug, sort_order).await {
        Ok(_) => Ok(super::redirect(&state.config.base_path,  "/admin/taxonomy")),
        Err(e) => {
            tracing::error!("更新分类失败: {e:?}");
            Ok(fail(&state.config.base_path, "更新分类失败，请重试"))
        }
    }
}

// ---------- 删除分类 / 标签 ----------

pub async fn delete_category(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<i64>,
    Form(form): Form<HashMap<String, String>>,
) -> Result<Response, AppError> {
    session::require_admin(&session).await?;
    session::verify_csrf(&session, form.get("csrf").map(String::as_str)).await?;
    match taxonomy::delete_category(&state.db, id).await {
        Ok(()) => Ok(super::redirect(&state.config.base_path,  "/admin/taxonomy")),
        Err(e) => {
            tracing::error!("删除分类失败: {e:?}");
            Ok(fail(&state.config.base_path, "删除分类失败，请重试"))
        }
    }
}

pub async fn create_tag(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<HashMap<String, String>>,
) -> Result<Response, AppError> {
    session::require_admin(&session).await?;
    session::verify_csrf(&session, form.get("csrf").map(String::as_str)).await?;
    let name = form.get("name").cloned().unwrap_or_default();
    if name.trim().is_empty() {
        return Ok(fail(&state.config.base_path, "标签名称不能为空"));
    }
    if name.trim().chars().count() > 5 {
        return Ok(fail(&state.config.base_path, "标签名称最多 5 个字"));
    }
    // ensure_tag 内部按 slug 去重：同名标签复用已有记录，不会冲突
    match taxonomy::ensure_tag(&state.db, &name).await {
        Ok(_) => Ok(super::redirect(&state.config.base_path,  "/admin/taxonomy")),
        Err(e) => {
            tracing::error!("创建标签失败: {e:?}");
            Ok(fail(&state.config.base_path, "创建标签失败，请重试"))
        }
    }
}

pub async fn delete_tag(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<i64>,
    Form(form): Form<HashMap<String, String>>,
) -> Result<Response, AppError> {
    session::require_admin(&session).await?;
    session::verify_csrf(&session, form.get("csrf").map(String::as_str)).await?;
    match taxonomy::delete_tag(&state.db, id).await {
        Ok(()) => Ok(super::redirect(&state.config.base_path,  "/admin/taxonomy")),
        Err(e) => {
            tracing::error!("删除标签失败: {e:?}");
            Ok(fail(&state.config.base_path, "删除标签失败，请重试"))
        }
    }
}

// ---------- 辅助 ----------

/// 表单 slug 解析：空则从名称 slugify（与文章/标签同一 `posts::slugify`），
/// 非空按用户输入原样使用（服务层负责唯一性检查）。
async fn parse_slug(form_slug: Option<&String>, name: &str) -> String {
    match form_slug.map(String::as_str).unwrap_or("").trim() {
        "" => posts::slugify(name).await,
        s => s.to_string(),
    }
}

/// 排序值解析：非法输入回落 0。
fn parse_sort_order(v: Option<&String>) -> i64 {
    v.and_then(|s| s.trim().parse::<i64>().ok()).unwrap_or(0)
}

/// 302 回列表并带 URL 编码的错误提示（消息含中文，直接拼 query 会丢非 ASCII）。
fn fail(base: &str, msg: &str) -> Response {
    super::redirect(base, &format!("/admin/taxonomy?msg={}", urlencode(msg)))
}

/// 查询参数值百分号编码（RFC 3986：仅保留 unreserved 字符）。
fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn categories_value(cats: &[Category], counts: &HashMap<i64, i64>) -> Value {
    json!(cats
        .iter()
        .map(|c| json!({
            "id": c.id,
            "slug": c.slug,
            "name": c.name,
            "sort_order": c.sort_order,
            "count": counts.get(&c.id).copied().unwrap_or(0),
        }))
        .collect::<Vec<_>>())
}

fn tags_value(tags: &[Tag], counts: &HashMap<i64, i64>) -> Value {
    json!(tags
        .iter()
        .map(|t| json!({ "id": t.id, "slug": t.slug, "name": t.name, "count": counts.get(&t.id).copied().unwrap_or(0) }))
        .collect::<Vec<_>>())
}

