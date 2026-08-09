//! API 备份导出：GET /api/backup 返回全量备份 zip 二进制。
//!
//! 鉴权：`require_admin_or_token`——后台 admin 会话或 `Authorization: Bearer <token>`
//! 二选一（与其余 API 端点一致）。响应 `Content-Type: application/zip` +
//! `Content-Disposition: attachment`，供 CI / 运维脚本拉取备份。

use crate::error::AppError;
use crate::services::backup;
use crate::AppState;
use axum::extract::State;
use axum::http::{HeaderMap, header};
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use tower_sessions::Session;

/// GET /api/backup：实时导出 zip 并以下载响应返回（备份内容随数据变化）。
pub async fn backup(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    super::require_admin_or_token(&state, &session, &headers).await?;
    let (zip_path, report) = backup::export_temp(&state.config.data_dir).await?;
    let bytes = tokio::fs::read(&zip_path).await.map_err(internal)?;
    let _ = std::fs::remove_file(&zip_path);
    let filename = format!("hancic-backup-{}.zip", Utc::now().format("%Y%m%d%H%M%S"));
    tracing::info!(
        "API 备份导出完成: {} 字节, {} 个文件",
        report.size,
        report.counts.files
    );
    Ok((
        [
            (header::CONTENT_TYPE, "application/zip".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        bytes,
    )
        .into_response())
}

fn internal(e: impl std::fmt::Display) -> AppError {
    AppError::Internal(e.to_string())
}
