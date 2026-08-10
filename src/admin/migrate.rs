//! 后台 Halo Markdown 迁移导入：上传 zip → 导入 → 报告页。
//!
//! 路由：
//!   GET  /admin/migrate        页面（上传表单 + 下载图片选项）
//!   POST /admin/migrate        multipart 接收 zip（≤500MB）→ 执行导入 → 报告页
//!
//! 鉴权约定同其他后台模块：GET 未登录 302 跳登录；POST 先 `require_admin`
//! 再过 CSRF。上传大小上限与备份恢复一致（500MB + 路由层 multipart 余量）。

use crate::services::migrate;
use crate::{session, AppState};
use axum::extract::{Multipart, OriginalUri, Query, State};
use axum::response::Response;
use serde_json::json;
use std::collections::HashMap;
use tower_sessions::Session;

/// 迁移上传 zip 大小上限（字节），路由层 `DefaultBodyLimit` 与业务校验共用。
pub const MIGRATE_MAX_BYTES: u64 = 500 * 1024 * 1024;

/// GET /admin/migrate：上传表单页（`?msg=` 展示操作错误，与 backup 一致）。
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
    super::render_admin(&state, "migrate.html", &ctx)
}

/// POST /admin/migrate：multipart 接收 zip + download_images 选项 → 落临时文件
/// → 导入 → 报告页（成功/失败均同页展示）。
pub async fn run(
    State(state): State<AppState>,
    session: Session,
    uri: OriginalUri,
    mut multipart: Multipart,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect(&state.config.base_path,  "/admin/login");
    }
    let mut csrf: Option<String> = None;
    let mut download_images = false;
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
            Some("download_images") => {
                download_images = field
                    .text()
                    .await
                    .map(|t| t == "on")
                    .unwrap_or(false);
            }
            Some("archive") => {
                // 分块读入并累计大小，避免整包直接撑爆内存后再校验
                let mut bytes: Vec<u8> = Vec::new();
                loop {
                    match field.chunk().await {
                        Ok(Some(chunk)) => {
                            total += chunk.len() as u64;
                            if total > MIGRATE_MAX_BYTES {
                                return redirect_msg(&state.config.base_path, "迁移包超过 500MB 上限");
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
        return redirect_msg(&state.config.base_path, "未收到 zip 文件，请选择 Halo 导出包");
    };
    if bytes.is_empty() {
        return redirect_msg(&state.config.base_path, "迁移包为空");
    }

    // 落临时文件交给服务层（zip 需 seek 定位中央目录）
    let zip_path = std::env::temp_dir().join(format!(
        "hancic-migrate-{}.zip",
        uuid::Uuid::new_v4()
    ));
    if std::fs::write(&zip_path, &bytes).is_err() {
        return redirect_msg(&state.config.base_path, "写入临时文件失败，请重试");
    }
    match migrate::import_halo_zip(&state.db, &state.config.data_dir, &zip_path, download_images)
        .await
    {
        Ok(report) => {
            tracing::info!(
                "Halo 迁移导入完成: 新建 {}, 跳过 {}, 图片下载 {} 失败 {}",
                report.posts_created,
                report.posts_skipped,
                report.images_downloaded,
                report.images_failed
            );
            let _ = std::fs::remove_file(&zip_path);
            render_result(&state, &session, uri.path(), report).await
        }
        Err(e) => {
            tracing::error!("Halo 迁移导入失败: {e:?}");
            let _ = std::fs::remove_file(&zip_path);
            redirect_msg(&state.config.base_path, e.message())
        }
    }
}

// ---------- 渲染 ----------

/// 导入报告页：计数汇总 + 失败明细。
async fn render_result(
    state: &AppState,
    session: &Session,
    path: &str,
    report: migrate::ImportReport,
) -> Response {
    let (mut ctx, _csrf) = super::base_ctx(state, session, path).await;
    ctx.insert(
        "result",
        &json!({
            "posts_created": report.posts_created,
            "posts_skipped": report.posts_skipped,
            "categories": report.categories,
            "tags": report.tags,
            "images_downloaded": report.images_downloaded,
            "images_failed": report.images_failed,
        }),
    );
    ctx.insert("failures", &json!(report.failures));
    super::render_admin(state, "migrate.html", &ctx)
}

/// 302 回迁移页并带 URL 编码的错误提示（消息含中文，直接拼 query 会丢非 ASCII）。
fn redirect_msg(base: &str, msg: &str) -> Response {
    super::redirect(base, &format!("/admin/migrate?msg={}", urlencode(msg)))
}

/// 查询参数值百分号编码（RFC 3986：仅保留 unreserved 字符；与 backup 同款）。
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
