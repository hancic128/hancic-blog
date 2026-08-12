//! 后台备份与恢复：导出 zip 下载 / 上传 zip 恢复。
//!
//! 路由：
//!   GET  /admin/backup          页面（导出按钮 + 上传恢复表单）
//!   POST /admin/backup/export   导出 zip，Content-Disposition 下载
//!   POST /admin/backup/restore  上传 zip（≤500MB）→ 校验 → 执行 → 结果页
//!
//! 鉴权约定同其他后台模块：GET 未登录 302 跳登录；POST 先 `require_admin`
//! 再过 CSRF。restore 会替换 hancic.db（含会话表），当前登录会话可能随之
//! 失效，且连接池仍指向改名保留的旧库，故结果页提示「重启服务」。
//!
//! 导出在 handler 内实时生成到临时文件再下发（备份内容随数据变化，
//! 不预生成持久文件）；大 zip 直接读入内存下发，管理操作可接受。

use crate::db::Db;
use crate::services::backup;
use crate::{session, AppState};
use axum::extract::{Form, Multipart, OriginalUri, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::Row;
use std::collections::HashMap;
use tower_sessions::Session;

/// 恢复上传 zip 大小上限（字节），路由层 `DefaultBodyLimit` 与业务校验共用。
pub const RESTORE_MAX_BYTES: u64 = 500 * 1024 * 1024;

/// 写入一条备份/恢复操作记录（失败不影响主流程）。
async fn log_backup(db: &Db, kind: &str, status: &str, size: i64, files: i64, note: &str) {
    let _ = sqlx::query(
        "INSERT INTO backup_logs(kind, status, size, files, note) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(kind)
    .bind(status)
    .bind(size)
    .bind(files)
    .bind(note)
    .execute(db)
    .await;
}

/// 最近备份记录（时间倒序前 20 条）。
async fn recent_logs(db: &Db) -> Vec<Value> {
    let rows = sqlx::query(
        "SELECT kind, status, size, files, note, created_at FROM backup_logs \
         ORDER BY created_at DESC, id DESC LIMIT 20",
    )
    .fetch_all(db)
    .await
    .unwrap_or_default();
    rows.into_iter()
        .map(|r| {
            json!({
                "kind": r.get::<String, _>("kind"),
                "status": r.get::<String, _>("status"),
                "size": r.get::<i64, _>("size"),
                "files": r.get::<i64, _>("files"),
                "note": r.get::<String, _>("note"),
                "created_at": super::format_local(chrono::DateTime::parse_from_rfc3339(
                    &r.get::<String, _>("created_at")
                ).map(|t| t.with_timezone(&chrono::Utc)).unwrap_or_else(|_| chrono::Utc::now())),
            })
        })
        .collect()
}

// ---------- 页面 ----------

/// GET /admin/backup：导出按钮 + 恢复上传表单（`?msg=` 展示操作结果/错误）。
pub async fn page(
    State(state): State<AppState>,
    session: Session,
    Query(query): Query<HashMap<String, String>>,
    uri: OriginalUri,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect(&state.config.base_path,  "/admin/login");
    }
    let (mut ctx, _csrf) = super::base_ctx(&state, &session, uri.path()).await;
    ctx.insert(
        "error_msg",
        &query.get("msg").map(String::as_str).unwrap_or(""),
    );
    ctx.insert("backup_logs", &recent_logs(&state.db).await);
    super::render_admin(&state, "backup.html", &ctx)
}

// ---------- 导出 ----------

/// POST /admin/backup/export：实时导出 zip 并以下载响应返回。
pub async fn export(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect(&state.config.base_path,  "/admin/login");
    }
    if session::verify_csrf(&session, form.get("csrf").map(String::as_str))
        .await
        .is_err()
    {
        return redirect_msg(&state.config.base_path, "安全校验失败，请刷新页面后重试");
    }
    let (zip_path, report) = match backup::export_temp(&state.config.data_dir).await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("导出备份失败: {e:?}");
            return redirect_msg(&state.config.base_path, "备份失败，请重试");
        }
    };
    let bytes = match tokio::fs::read(&zip_path).await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("读取备份 zip 失败: {e}");
            let _ = std::fs::remove_file(&zip_path);
            return redirect_msg(&state.config.base_path, "备份失败，请重试");
        }
    };
    let _ = std::fs::remove_file(&zip_path);
    let filename = format!("hancic-backup-{}.zip", Utc::now().format("%Y%m%d%H%M%S"));
    tracing::info!(
        "备份导出完成: {} 字节, {} 个文件",
        report.size,
        report.counts.files
    );
    log_backup(
        &state.db,
        "export",
        "ok",
        bytes.len() as i64,
        report.counts.files as i64,
        "",
    )
    .await;
    (
        [
            (header::CONTENT_TYPE, "application/zip".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        bytes,
    )
        .into_response()
}

// ---------- 恢复 ----------

/// POST /admin/backup/restore：multipart 接收 zip → 落临时文件 → restore →
/// 结果页（成功/失败均同页展示）。
pub async fn restore(
    State(state): State<AppState>,
    session: Session,
    uri: OriginalUri,
    mut multipart: Multipart,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect(&state.config.base_path,  "/admin/login");
    }
    let mut csrf: Option<String> = None;
    let mut zip_bytes: Option<Vec<u8>> = None;
    let mut total: u64 = 0;
    while let Some(mut field) = match multipart.next_field().await {
        Ok(f) => f,
        Err(_) => return redirect_msg(&state.config.base_path, "读取上传失败：文件过大或格式错误"),
    } {
        match field.name() {
            Some("csrf") => {
                csrf = field.text().await.ok();
            }
            Some("backup") => {
                // 分块读入并累计大小，避免整包直接撑爆内存后再校验
                let mut bytes: Vec<u8> = Vec::new();
                loop {
                    match field.chunk().await {
                        Ok(Some(chunk)) => {
                            total += chunk.len() as u64;
                            if total > RESTORE_MAX_BYTES {
                                return redirect_msg(&state.config.base_path, "备份包超过 500MB 上限");
                            }
                            bytes.extend_from_slice(&chunk);
                        }
                        Ok(None) => break,
                        Err(_) => return redirect_msg(&state.config.base_path, "读取上传失败：文件过大或格式错误"),
                    }
                }
                zip_bytes = Some(bytes);
            }
            _ => {}
        }
    }
    if session::verify_csrf(&session, csrf.as_deref())
        .await
        .is_err()
    {
        return redirect_msg(&state.config.base_path, "安全校验失败，请刷新页面后重试");
    }
    let Some(bytes) = zip_bytes else {
        return redirect_msg(&state.config.base_path, "未收到备份包，请选择 zip 文件");
    };
    if bytes.is_empty() {
        return redirect_msg(&state.config.base_path, "备份包为空");
    }

    // 落临时文件交给服务层（zip 需 seek 定位中央目录）
    let zip_path = std::env::temp_dir().join(format!(
        "hancic-restore-{}.zip",
        uuid::Uuid::new_v4()
    ));
    if std::fs::write(&zip_path, &bytes).is_err() {
        return redirect_msg(&state.config.base_path, "写入临时文件失败，请重试");
    }
    match backup::restore(&state.config.data_dir, &zip_path).await {
        Ok(report) => {
            tracing::info!("备份恢复完成: {} 个文件", report.files);
            log_backup(&state.db, "restore", "ok", 0, report.files as i64, "").await;
            let _ = std::fs::remove_file(&zip_path);
            render_result(&state, &session, uri.path(), report, "").await
        }
        Err(e) => {
            tracing::error!("备份恢复失败: {e:?}");
            log_backup(&state.db, "restore", "failed", 0, 0, e.message()).await;
            let _ = std::fs::remove_file(&zip_path);
            redirect_msg(&state.config.base_path, e.message())
        }
    }
}

// ---------- 渲染 ----------

/// 恢复结果页：报告 + 重启提示；`msg` 为恢复成功时的附加提示（当前无）。
async fn render_result(
    state: &AppState,
    session: &Session,
    path: &str,
    report: backup::RestoreReport,
    msg: &str,
) -> Response {
    let (mut ctx, _csrf) = super::base_ctx(state, session, path).await;
    ctx.insert("error_msg", msg);
    ctx.insert(
        "result",
        &json!({
            "files": report.files,
            "uploads": report.uploads,
            "themes": report.themes,
            "config": report.config,
            "db": report.db,
            "backup_dir": report.backup_dir.display().to_string(),
        }),
    );
    super::render_admin(state, "backup.html", &ctx)
}

/// 302 回备份页并带 URL 编码的错误提示（消息含中文，直接拼 query 会丢非 ASCII）。
fn redirect_msg(base: &str, msg: &str) -> Response {
    super::redirect(base, &format!("/admin/backup?msg={}", urlencode(msg)))
}

/// 查询参数值百分号编码（RFC 3986：仅保留 unreserved 字符；与 tokens 同款）。
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

