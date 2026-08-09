//! API Bearer 鉴权：从 `Authorization: Bearer <raw>` 提取明文并调 `tokens::verify`。
//!
//! 失败统一 401 + `{"error":{"code":401,"message":"无效的 API Token"}}`
//! （错误体由 `AppError::Unauthorized` 的 `IntoResponse` 统一产出）。
//! 端点鉴权入口是 `api::require_admin_or_token`（session 或 Bearer 二选一），
//! `require_token` 提供仅 Bearer 的 handler extractor 形态。

use crate::error::AppError;
use crate::services::tokens;
use crate::AppState;
use axum::extract::State;
use axum::http::header;
use axum::http::HeaderMap;

/// 仅凭 Bearer token 鉴权（handler extractor 形态）。
pub async fn require_token(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(), AppError> {
    verify_bearer(&state, &headers).await
}

/// 提取 `Authorization: Bearer <raw>` 的明文；无头/非 Bearer 格式/空明文返回 None。
/// Scheme 按 RFC 9110 大小写不敏感（支持 `Bearer`/`bearer`/`BEARER` 等任意大小写）。
/// 头值形如 `Bearer hc_...`（空格分隔，无冒号），故以首个空格切分。
pub fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, raw) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let raw = raw.trim();
    if raw.is_empty() {
        None
    } else {
        Some(raw)
    }
}

/// 校验 Bearer 明文：格式无效或未命中未吊销 Token 均 401。
pub(crate) async fn verify_bearer(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(), AppError> {
    let raw = bearer_token(headers).ok_or_else(unauthorized)?;
    if tokens::verify(&state.db, raw).await {
        Ok(())
    } else {
        Err(unauthorized())
    }
}

fn unauthorized() -> AppError {
    AppError::Unauthorized("无效的 API Token".into())
}
