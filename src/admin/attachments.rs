//! 后台附件库：列表（kind 筛选 + 分页 + 卡片网格）、删除与上传页。
//!
//! 鉴权约定同文章管理：GET 页面未登录 302 跳登录；POST 先 `require_admin`
//! （未登录 401 JSON）再过 CSRF。列表按 `kind`（image|video|file，非法值忽略）
//! 参数化筛选，走 `uploads::list_attachments`；删除调 `uploads::delete_attachment`
//! （删磁盘文件 + DB 行，不存在视为成功）。上传页内嵌拖拽/多选组件，走
//! `/api/uploads`（session 鉴权），成功后跳回列表。

use crate::error::AppError;
use crate::models::AttachmentKind;
use crate::services::uploads;
use crate::{session, AppState};
use axum::extract::{Form, OriginalUri, Path, Query, State};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use std::collections::HashMap;
use tower_sessions::Session;

/// 列表每页条数。
const PAGE_SIZE: i64 = 20;

// ---------- 列表 ----------

/// 附件 JSON 列表（设置页 Logo 选择器用）：按 kind 筛选，返回 id/url/名称。
/// 鉴权同列表页（未登录 302 登录页）。
pub async fn api_list(
    State(state): State<AppState>,
    session: Session,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect(&state.config.base_path, "/admin/login");
    }
    let kind = match query.get("kind").map(String::as_str).unwrap_or("") {
        "image" => Some(AttachmentKind::Image),
        "video" => Some(AttachmentKind::Video),
        "file" => Some(AttachmentKind::File),
        _ => None,
    };
    let (items, _) = match uploads::list_attachments(&state.db, kind, false, None, 1, 200).await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("附件 JSON 列表查询失败: {e:?}");
            return axum::Json(json!([])).into_response();
        }
    };
    let base = state.config.base_path.clone();
    axum::Json(json!(items
        .iter()
        .map(|a| json!({
            "id": a.id,
            "url": format!("{base}/uploads/{}", a.path),
            "name": crate::util::percent_decode(&a.orig_name),
        }))
        .collect::<Vec<_>>()))
    .into_response()
}

pub async fn list(
    State(state): State<AppState>,
    session: Session,
    Query(query): Query<HashMap<String, String>>,
    uri: OriginalUri,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect(&state.config.base_path, "/admin/login");
    }
    let kind = match query.get("kind").map(String::as_str).unwrap_or("") {
        "image" => Some(AttachmentKind::Image),
        "video" => Some(AttachmentKind::Video),
        "file" => Some(AttachmentKind::File),
        _ => None,
    };
    let asc = query.get("order").map(String::as_str).unwrap_or("desc") == "asc";
    let q = query.get("q").map(String::as_str);
    let page = query
        .get("page")
        .and_then(|p| p.parse::<i64>().ok())
        .filter(|&p| p > 0)
        .unwrap_or(1);
    let (items, total) = match uploads::list_attachments(&state.db, kind, asc, q, page, PAGE_SIZE).await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("后台附件列表查询失败: {e:?}");
            (vec![], 0)
        }
    };
    let (mut ctx, _csrf) = super::base_ctx(&state, &session, uri.path()).await;
    ctx.insert("attachments", &attachment_list_value(&state.config.base_path, &items));
    ctx.insert("total", &total);
    ctx.insert("page", &page);
    ctx.insert("total_pages", &((total + PAGE_SIZE - 1) / PAGE_SIZE).max(1));
    // filters.kind/order 仅为模板选中态，非法值归一为空（不原样回显 query）
    ctx.insert(
        "filters",
        &json!({
            "kind": kind.map(|k| k.to_str()).unwrap_or(""),
            "order": if asc { "asc" } else { "desc" },
            "q": q.map(|s| s.to_string()).unwrap_or_default(),
        }),
    );
    super::render_admin(&state, "attachments.html", &ctx)
}

/// 卡片 JSON：kind 供模板分支，url 指向前台 /uploads 静态路径，size 人类可读。
fn attachment_list_value(base: &str, items: &[crate::models::Attachment]) -> Value {
    json!(items
        .iter()
        .map(|a| json!({
            "id": a.id,
            "kind": a.kind.to_str(),
            "orig_name": crate::util::percent_decode(&a.orig_name),
            "mime": a.mime,
            "size": human_size(a.size),
            "url": format!("{base}/uploads/{}", a.path),
            "created_at": super::format_local(a.created_at),
        }))
        .collect::<Vec<_>>())
}

/// 字节数 → 人类可读（B / KB / MB，取一位小数）。
fn human_size(bytes: i64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

// ---------- 删除 ----------

pub async fn delete(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<i64>,
    Form(form): Form<HashMap<String, String>>,
) -> Result<Response, AppError> {
    session::require_admin(&session).await?;
    session::verify_csrf(&session, form.get("csrf").map(String::as_str)).await?;
    let uploads_dir = state.config.data_dir.join("uploads");
    match uploads::delete_attachment(&state.db, &uploads_dir, id).await {
        Ok(()) => Ok(super::redirect(&state.config.base_path, "/admin/attachments")),
        Err(e) => {
            tracing::error!("删除附件失败: {e:?}");
            Ok(super::redirect(&state.config.base_path, "/admin/attachments"))
        }
    }
}

// ---------- 上传页 ----------

pub async fn upload_page(
    State(state): State<AppState>,
    session: Session,
    uri: OriginalUri,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect(&state.config.base_path, "/admin/login");
    }
    let (mut ctx, _csrf) = super::base_ctx(&state, &session, uri.path()).await;
    ctx.insert("page_mode", "upload");
    super::render_admin(&state, "attachments.html", &ctx)
}
