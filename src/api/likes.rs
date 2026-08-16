use crate::config::Config;
use crate::error::AppError;
use crate::models::LikeContentType;
use crate::services::likes::{self, LikeStatus};
use crate::AppState;
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::Arc;

const VISITOR_COOKIE: &str = "visitor";
const VISITOR_COOKIE_MAX_AGE: u64 = 15552000;

#[derive(Deserialize)]
pub struct LikeQuery {
    content_type: LikeContentType,
    content_id: i64,
}

#[derive(Deserialize)]
pub struct ToggleLikeInput {
    content_type: LikeContentType,
    content_id: i64,
}

pub async fn status(
    State(state): State<AppState>,
    Query(query): Query<LikeQuery>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let (visitor_id, set_cookie) = ensure_visitor_cookie(&state.config, &headers);
    let data = likes::like_status(&state.db, query.content_type, query.content_id, &visitor_id).await?;
    Ok(like_response(data, set_cookie))
}

pub async fn toggle(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ToggleLikeInput>,
) -> Result<Response, AppError> {
    let (visitor_id, set_cookie) = ensure_visitor_cookie(&state.config, &headers);
    let ip_hash = likes::hash_client_hint(client_ip(&headers));
    let ua_hash = likes::hash_client_hint(user_agent(&headers));
    let rate_key = format!("{visitor_id}:{ip_hash}:{ua_hash}");
    if !state.like_rate_limiter.check(&rate_key) {
        return Err(AppError::TooManyRequests("请求过于频繁，请稍后再试".into()));
    }
    let data = likes::toggle_like(
        &state.db,
        input.content_type,
        input.content_id,
        &visitor_id,
        &ip_hash,
        &ua_hash,
    )
    .await?;
    Ok(like_response(data, set_cookie))
}

fn like_response(data: LikeStatus, set_cookie: Option<HeaderValue>) -> Response {
    let mut res = Json(json!({ "data": data })).into_response();
    if let Some(cookie) = set_cookie {
        res.headers_mut().append(header::SET_COOKIE, cookie);
    }
    res
}

pub fn ensure_visitor_cookie(
    config: &Arc<Config>,
    headers: &HeaderMap,
) -> (String, Option<HeaderValue>) {
    if let Some(existing) = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|raw| existing_visitor_id(config, raw))
    {
        return (existing, None);
    }
    let visitor_id = uuid::Uuid::new_v4().to_string();
    let cookie = signed_cookie_header(config, &visitor_id);
    (visitor_id, Some(cookie))
}

fn signed_cookie_header(config: &Arc<Config>, visitor_id: &str) -> HeaderValue {
    let signed = format!("{visitor_id}.{}", visitor_signature(config, visitor_id));
    HeaderValue::from_str(&format!(
        "{VISITOR_COOKIE}={signed}; Path=/; HttpOnly; SameSite=Lax; Max-Age={VISITOR_COOKIE_MAX_AGE}"
    ))
    .expect("visitor cookie header should be valid")
}

fn existing_visitor_id(config: &Arc<Config>, cookie: &str) -> Option<String> {
    cookie.split(';').find_map(|part| {
        let part = part.trim();
        let (name, value) = part.split_once('=')?;
        if name != VISITOR_COOKIE {
            return None;
        }
        let value = value.trim();
        let (visitor_id, signature) = value.rsplit_once('.')?;
        uuid::Uuid::parse_str(visitor_id).ok()?;
        let expected = visitor_signature(config, visitor_id);
        if signature == expected {
            Some(visitor_id.to_string())
        } else {
            None
        }
    })
}

fn visitor_signature(config: &Arc<Config>, visitor_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(config.data_dir.to_string_lossy().as_bytes());
    hasher.update(b":visitor:");
    hasher.update(config.site_name.as_bytes());
    hasher.update(b":");
    hasher.update(visitor_id.as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn client_ip(headers: &HeaderMap) -> &str {
    headers
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
        .or_else(|| {
            headers
                .get("x-forwarded-for")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.split(',').next())
                .map(str::trim)
        })
        .unwrap_or("")
}

fn user_agent(headers: &HeaderMap) -> &str {
    headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
}
