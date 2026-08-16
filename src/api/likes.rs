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
    let (visitor_id, set_cookie) = ensure_visitor_cookie(&headers);
    let data = likes::like_status(&state.db, query.content_type, query.content_id, &visitor_id).await?;
    Ok(like_response(data, set_cookie))
}

pub async fn toggle(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ToggleLikeInput>,
) -> Result<Response, AppError> {
    let (visitor_id, set_cookie) = ensure_visitor_cookie(&headers);
    let ip_hash = likes::hash_client_hint(client_ip(&headers));
    let ua_hash = likes::hash_client_hint(user_agent(&headers));
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

pub fn ensure_visitor_cookie(headers: &HeaderMap) -> (String, Option<HeaderValue>) {
    if let Some(existing) = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(existing_visitor_id)
    {
        return (existing, None);
    }
    let visitor_id = uuid::Uuid::new_v4().to_string();
    let cookie = HeaderValue::from_str(&format!(
        "{VISITOR_COOKIE}={visitor_id}; Path=/; HttpOnly; SameSite=Lax; Max-Age={VISITOR_COOKIE_MAX_AGE}"
    ))
    .expect("visitor cookie header should be valid");
    (visitor_id, Some(cookie))
}

fn existing_visitor_id(cookie: &str) -> Option<String> {
    cookie.split(';').find_map(|part| {
        let part = part.trim();
        let (name, value) = part.split_once('=')?;
        if name == VISITOR_COOKIE && !value.trim().is_empty() {
            Some(value.trim().to_string())
        } else {
            None
        }
    })
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
