use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug)]
pub enum AppError {
    NotFound(String),
    BadRequest(String),
    Unauthorized(String),
    Forbidden(String),
    TooManyRequests(String),
    Internal(String),
    Conflict(String),
}

impl AppError {
    /// 对应的 HTTP 状态码（前台错误页等内部模块使用）。
    pub(crate) fn status(&self) -> StatusCode {
        match self {
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            AppError::Forbidden(_) => StatusCode::FORBIDDEN,
            AppError::TooManyRequests(_) => StatusCode::TOO_MANY_REQUESTS,
            AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::Conflict(_) => StatusCode::CONFLICT,
        }
    }
    /// 错误消息（前台错误页、二进制启动错误等使用）。
    pub fn message(&self) -> &str {
        match self {
            AppError::NotFound(m)
            | AppError::BadRequest(m)
            | AppError::Unauthorized(m)
            | AppError::Forbidden(m)
            | AppError::TooManyRequests(m)
            | AppError::Internal(m)
            | AppError::Conflict(m) => m,
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status();
        let body = Json(json!({
            "error": { "code": status.as_u16(), "message": self.message() }
        }));
        (status, body).into_response()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        tracing::error!("sqlx error: {e}");
        AppError::Internal("数据库错误".into())
    }
}

pub type AppResult<T> = Result<T, AppError>;
