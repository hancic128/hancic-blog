//! 后台 API Token 管理：列表 / 生成（明文一次性展示）/ 吊销。
//!
//! 路由：
//!   GET  /admin/tokens                  列表（名称 / 前缀 / 创建时间 / 状态）
//!   POST /admin/tokens                  生成（name）→ 302 到 `/admin/tokens/{id}/created`
//!   GET  /admin/tokens/{id}/created     明文仅显示一次（读后即删）
//!   POST /admin/tokens/{id}/revoke      吊销
//!
//! 明文生命周期：`tokens::generate` 返回明文后写入 `AppState::token_plain`
//! （内存暂存，5 分钟 TTL），created 页读取即删；刷新或二次访问不再可得，
//! 显示「仅显示一次」兜底提示。库中仅存 sha256 hex，明文不落库不落日志。
//!
//! 鉴权约定同其他后台模块：GET 未登录 302 跳登录；POST 先 `require_admin`
//! 再过 CSRF。列表「前缀」展示为库存哈希前 10 位（明文不可复原，短哈希
//! 作唯一标识），与 git 短哈希同思路。

use crate::error::AppError;
use crate::models::ApiToken;
use crate::services::tokens;
use crate::{session, AppState};
use axum::extract::{Form, OriginalUri, Path, Query, State};
use axum::response::Response;
use serde_json::{Value, json};
use std::collections::HashMap;
use tower_sessions::Session;

/// 列表页展示的哈希前缀长度。
const PREFIX_LEN: usize = 10;

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
    let tokens = tokens::list(&state.db).await.unwrap_or_default();
    let (mut ctx, _csrf) = super::base_ctx(&state, &session, uri.path()).await;
    ctx.insert("tokens", &tokens_value(&tokens));
    // POST 失败回显的错误提示（`?msg=`）
    ctx.insert(
        "error_msg",
        &query.get("msg").map(String::as_str).unwrap_or(""),
    );
    super::render_admin(&state, "tokens.html", &ctx)
}

// ---------- 生成 ----------

pub async fn create(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<HashMap<String, String>>,
) -> Result<Response, AppError> {
    session::require_admin(&session).await?;
    session::verify_csrf(&session, form.get("csrf").map(String::as_str)).await?;
    let name = form.get("name").cloned().unwrap_or_default();
    if name.trim().is_empty() {
        return Ok(fail(&state.config.base_path, "Token 名称不能为空"));
    }
    match tokens::generate(&state.db, name.trim()).await {
        Ok((token, plain)) => {
            // 明文入内存暂存，created 页读取即删（仅显示一次）
            state.token_plain.put(token.id, plain);
            Ok(super::redirect(&state.config.base_path, &format!(
                "/admin/tokens/{}/created",
                token.id
            )))
        }
        Err(e) => {
            tracing::error!("生成 Token 失败: {e:?}");
            Ok(fail(&state.config.base_path, "生成 Token 失败，请重试"))
        }
    }
}

// ---------- 明文展示（仅一次） ----------

pub async fn created_page(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<i64>,
    uri: OriginalUri,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect(&state.config.base_path,  "/admin/login");
    }
    let Some(token) = tokens::get_by_id(&state.db, id).await.ok().flatten() else {
        return super::redirect(&state.config.base_path,  "/admin/tokens");
    };
    // 读取即删：刷新/二次访问拿到的是已失效提示
    let (plain, expired) = match state.token_plain.take(id) {
        Some(raw) => (raw, false),
        None => (String::new(), true),
    };
    let (mut ctx, _csrf) = super::base_ctx(&state, &session, uri.path()).await;
    ctx.insert("token_name", &token.name);
    ctx.insert("plain", &plain);
    ctx.insert("expired", &expired);
    super::render_admin(&state, "tokens_created.html", &ctx)
}

// ---------- 吊销 ----------

pub async fn revoke(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<i64>,
    Form(form): Form<HashMap<String, String>>,
) -> Result<Response, AppError> {
    session::require_admin(&session).await?;
    session::verify_csrf(&session, form.get("csrf").map(String::as_str)).await?;
    match tokens::revoke(&state.db, id).await {
        Ok(()) => Ok(super::redirect(&state.config.base_path,  "/admin/tokens")),
        Err(e) => {
            tracing::error!("吊销 Token 失败: {e:?}");
            Ok(fail(&state.config.base_path, "吊销 Token 失败，请重试"))
        }
    }
}

// ---------- 辅助 ----------

/// 302 回列表并带 URL 编码的错误提示（消息含中文，直接拼 query 会丢非 ASCII）。
fn fail(base: &str, msg: &str) -> Response {
    super::redirect(base, &format!("/admin/tokens?msg={}", urlencode(msg)))
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

fn tokens_value(tokens: &[ApiToken]) -> Value {
    json!(tokens
        .iter()
        .map(|t| json!({
            "id": t.id,
            "name": t.name,
            "prefix": &t.token_hash[..PREFIX_LEN],
            "created_at": super::format_local(t.created_at),
            "status": if t.revoked_at.is_some() { "revoked" } else { "active" },
        }))
        .collect::<Vec<_>>())
}
