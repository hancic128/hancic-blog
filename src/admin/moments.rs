//! 后台说说管理：朋友圈式发布框（上传 + 缩略预览）与列表/删除。
//!
//! 鉴权约定同文章管理：GET 页面未登录 302 跳登录；POST 一律先 `require_admin`
//! （未登录 401 JSON）再过 CSRF。列表取 `moments::list_moments` 分页 + 每条
//! `list_moment_attachments` 拼缩略（图片 `<img src="/uploads/{path}">`，视频
//! 图标，文件卡片）。发布框上传走 `/api/uploads`（session 鉴权），前端把返回的
//! attachment id 追加进隐藏字段 `attachment_ids`（逗号分隔）随表单提交。

use crate::error::AppError;
use crate::models::{Attachment, Moment};
use crate::services::moments;
use crate::{session, AppState};
use axum::extract::{Form, OriginalUri, Path, Query, State};
use axum::response::Response;
use serde_json::{Value, json};
use std::collections::HashMap;
use tower_sessions::Session;

/// 列表每页条数。
/// 列表每页条数（说说默认 10 条/页）。
const PAGE_SIZE: i64 = 10;

// ---------- 列表 ----------

pub async fn list(
    State(state): State<AppState>,
    session: Session,
    Query(query): Query<HashMap<String, String>>,
    uri: OriginalUri,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect(&state.config.base_path, "/admin/login");
    }
    let page = query
        .get("page")
        .and_then(|p| p.parse::<i64>().ok())
        .filter(|&p| p > 0)
        .unwrap_or(1);
    let month = query.get("month").filter(|m| !m.is_empty()).cloned();
    let q = query
        .get("q")
        .map(String::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let asc = query.get("order").map(String::as_str).unwrap_or("desc") == "asc";
    let (items, total) = match moments::list_moments(&state.db, month.as_deref(), asc, Some(&q), page, PAGE_SIZE).await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("后台说说列表查询失败: {e:?}");
            (vec![], 0)
        }
    };
    let (mut ctx, _csrf) = super::base_ctx(&state, &session, uri.path()).await;
    ctx.insert("moments", &moment_list_value(&state, &items).await);
    ctx.insert("flash_msg", &query.get("msg").map(String::as_str).unwrap_or(""));
    ctx.insert("total", &total);
    ctx.insert("page", &page);
    ctx.insert("total_pages", &((total + PAGE_SIZE - 1) / PAGE_SIZE).max(1));
    let months = moments::month_list(&state.db).await.unwrap_or_default();
    ctx.insert(
        "months",
        &json!(months.iter().map(|m| json!({ "month": m })).collect::<Vec<_>>()),
    );
    ctx.insert(
        "filters",
        &json!({
            "month": month.unwrap_or_default(),
            "order": if asc { "asc" } else { "desc" },
            "q": q,
        }),
    );
    super::render_admin(&state, "moments.html", &ctx)
}

/// 列表行 JSON：每条附上附件缩略信息（kind 供模板分支，url 指向前台 /uploads 静态路径）。
async fn moment_list_value(state: &AppState, items: &[Moment]) -> Value {
    let mut out = Vec::new();
    for m in items {
        let atts = moments::list_moment_attachments(&state.db, m.id)
            .await
            .unwrap_or_default();
        let attachments: Vec<Value> = atts
            .iter()
            .map(|(a, _)| attachment_value(&state.config.base_path, a))
            .collect();
        out.push(json!({
            "id": m.id,
            "content": m.content,
            "created_at": super::format_local(m.created_at),
            "like_count": m.like_count,
            "attachments": attachments,
            // 编辑表单 JS 用：序列化字符串注入 data-attachments 属性（tera autoescape 保证安全）
            "attachments_json": serde_json::to_string(&attachments).unwrap_or_else(|_| "[]".into()),
        }));
    }
    json!(out)
}

fn attachment_value(base: &str, a: &Attachment) -> Value {
    json!({
        "id": a.id,
        "kind": a.kind.to_str(),
        "orig_name": crate::util::percent_decode(&a.orig_name),
        "url": format!("{base}/uploads/{}", a.path),
    })
}

/// 简单百分号编码（保留 ASCII 字母数字与 `-_.~`），用于 redirect 查询串携带中文提示。
fn encode_query(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
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

// ---------- 发布 ----------

pub async fn create(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<HashMap<String, String>>,
) -> Result<Response, AppError> {
    session::require_admin(&session).await?;
    session::verify_csrf(&session, form.get("csrf").map(String::as_str)).await?;
    let content = form.get("content").cloned().unwrap_or_default();
    // attachment_ids 逗号分隔，过滤空段与非法 id
    let attachment_ids = form
        .get("attachment_ids")
        .map(String::as_str)
        .unwrap_or("")
        .split(',')
        .filter_map(|s| s.trim().parse::<i64>().ok())
        .collect::<Vec<_>>();
    // 非空校验：说说至少要包含文字/图片/视频之一
    if content.trim().is_empty() && attachment_ids.is_empty() {
        return Ok(super::redirect(
            &state.config.base_path,
            &format!("/admin/moments?msg={}", encode_query("说说至少要包含文字或图片/视频")),
        ));
    }
    match moments::create_moment(&state.db, &content, &attachment_ids).await {
        Ok(_) => Ok(super::redirect(&state.config.base_path, "/admin/moments")),
        Err(e) => {
            tracing::error!("创建说说失败: {e:?}");
            Ok(super::redirect(&state.config.base_path, "/admin/moments"))
        }
    }
}

// ---------- 编辑 ----------

pub async fn update(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<i64>,
    Form(form): Form<HashMap<String, String>>,
) -> Result<Response, AppError> {
    session::require_admin(&session).await?;
    session::verify_csrf(&session, form.get("csrf").map(String::as_str)).await?;
    let content = form.get("content").cloned().unwrap_or_default();
    // attachment_ids 逗号分隔（编辑页提交完整保留列表：移除=删除，新增=新增）
    let attachment_ids = form
        .get("attachment_ids")
        .map(String::as_str)
        .unwrap_or("")
        .split(',')
        .filter_map(|s| s.trim().parse::<i64>().ok())
        .collect::<Vec<_>>();
    // 编辑后不能为空：文字为空且无附件则拒绝（附件列表随本次提交整体重建）
    if content.trim().is_empty() && attachment_ids.is_empty() {
        return Ok(super::redirect(
            &state.config.base_path,
            &format!("/admin/moments?msg={}", encode_query("说说至少要包含文字或图片/视频")),
        ));
    }
    match moments::update_moment_with_attachments(&state.db, id, &content, &attachment_ids).await {
        Ok(true) => Ok(super::redirect(&state.config.base_path, "/admin/moments")),
        Ok(false) => {
            tracing::warn!("编辑说说失败: 说说不存在 id={id}");
            Ok(super::redirect(&state.config.base_path, "/admin/moments"))
        }
        Err(e) => {
            tracing::error!("编辑说说失败: {e:?}");
            Ok(super::redirect(&state.config.base_path, "/admin/moments"))
        }
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
    match moments::delete_moment(&state.db, id).await {
        Ok(()) => Ok(super::redirect(&state.config.base_path, "/admin/moments")),
        Err(e) => {
            tracing::error!("删除说说失败: {e:?}");
            Ok(super::redirect(&state.config.base_path, "/admin/moments"))
        }
    }
}
