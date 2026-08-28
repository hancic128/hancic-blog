//! 后台系统设置：主题模式 + 时区 + 修改密码（独立菜单页「系统设置」）。
//!
//! 从站点设置页拆出：GET /admin/system 渲染；POST /admin/system/save 保存
//! 主题模式/时区（settings 键，前台即时生效）；POST /admin/system/password
//! 修改密码（校验逻辑同原设置页）。

use crate::{auth, session, AppState};
use axum::extract::{Form, OriginalUri, State};
use axum::response::Response;
use std::collections::HashMap;
use std::str::FromStr;
use tower_sessions::Session;

/// 设置表单字段（settings 表键名，与前台 `site_context` 读取一致）。
const FORM_KEYS: [&str; 3] = ["theme_mode", "timezone", "date_format"];

/// 允许的主题模式。
const THEME_MODES: [&str; 3] = ["auto", "light", "dark"];

/// 常用时区下拉选项（IANA 名，覆盖主要城市/时区；其他值校验仍走 chrono_tz）。
const TIMEZONES: [&str; 24] = [
    "Asia/Shanghai",
    "Asia/Tokyo",
    "Asia/Hong_Kong",
    "Asia/Taipei",
    "Asia/Singapore",
    "Asia/Seoul",
    "Asia/Bangkok",
    "Asia/Kuala_Lumpur",
    "Asia/Jakarta",
    "Asia/Dubai",
    "Asia/Kolkata",
    "Australia/Sydney",
    "Australia/Melbourne",
    "Pacific/Auckland",
    "Europe/London",
    "Europe/Paris",
    "Europe/Berlin",
    "Europe/Moscow",
    "America/New_York",
    "America/Chicago",
    "America/Denver",
    "America/Los_Angeles",
    "America/Sao_Paulo",
    "UTC",
];

// ---------- 页面 ----------

pub async fn page(
    State(state): State<AppState>,
    session: Session,
    uri: OriginalUri,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect(&state.config.base_path, "/admin/login");
    }
    render(&state, &session, uri.path(), None, "", "").await
}

// ---------- 保存系统设置 ----------

pub async fn save(
    State(state): State<AppState>,
    session: Session,
    uri: OriginalUri,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect(&state.config.base_path, "/admin/login");
    }
    if session::verify_csrf(&session, form.get("csrf").map(String::as_str))
        .await
        .is_err()
    {
        return render(
            &state,
            &session,
            uri.path(),
            Some(&form),
            "安全校验失败，请刷新页面后重试",
            "",
        )
        .await;
    }
    let errors = validate(&form);
    if !errors.is_empty() {
        return render(&state, &session, uri.path(), Some(&form), &errors.join("；"), "").await;
    }
    for key in FORM_KEYS {
        if let Some(value) = form.get(key) {
            if let Err(e) = crate::services::settings::set(&state.db, key, value.trim()).await {
                tracing::error!("保存系统设置 {key} 失败: {e:?}");
                return render(
                    &state,
                    &session,
                    uri.path(),
                    Some(&form),
                    "保存失败，请重试",
                    "",
                )
                .await;
            }
        }
    }
    super::redirect(&state.config.base_path, "/admin/system")
}

// ---------- 修改密码 ----------

pub async fn password(
    State(state): State<AppState>,
    session: Session,
    uri: OriginalUri,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect(&state.config.base_path, "/admin/login");
    }
    if session::verify_csrf(&session, form.get("csrf").map(String::as_str))
        .await
        .is_err()
    {
        return render(&state, &session, uri.path(), None, "", "安全校验失败，请刷新页面后重试").await;
    }
    let old = form.get("old_password").map(String::as_str).unwrap_or("");
    let new = form.get("new_password").map(String::as_str).unwrap_or("");
    let confirm = form.get("confirm").map(String::as_str).unwrap_or("");

    // 旧密码校验：argon2 为 CPU 密集操作，走 spawn_blocking（与登录一致）
    let Some(hash) = auth::get_password_hash(&state.db).await.ok().flatten() else {
        return render(&state, &session, uri.path(), None, "", "尚未设置管理员密码，请通过安装流程初始化")
            .await;
    };
    let new_same_as_old = new == old;
    let old_owned = old.to_string();
    let old_ok = tokio::task::spawn_blocking(move || auth::verify_password(&old_owned, &hash))
        .await
        .unwrap_or(false);
    if !old_ok {
        return render(&state, &session, uri.path(), None, "", "旧密码不正确").await;
    }
    if let Err(msg) = auth::validate_password_strength(new) {
        return render(&state, &session, uri.path(), None, "", msg.as_str()).await;
    }
    if new_same_as_old {
        return render(&state, &session, uri.path(), None, "", "新密码不能与旧密码相同").await;
    }
    if new != confirm {
        return render(&state, &session, uri.path(), None, "", "两次输入的新密码不一致").await;
    }
    match auth::set_password(&state.db, new).await {
        Ok(()) => {
            // 改密后失效全部既有会话（多端登录一并踢出）；当前会话 flush
            // 让中间件下发清除 cookie，随后跳登录页强制重新登录（I3）。
            let _ = sqlx::query(&format!("DELETE FROM {}", session::SESSION_TABLE))
                .execute(&state.db)
                .await;
            let _ = session::logout(&session).await;
            super::redirect(&state.config.base_path, "/admin/login")
        }
        Err(e) => {
            tracing::error!("修改密码失败: {e:?}");
            render(&state, &session, uri.path(), None, "", "密码修改失败，请重试").await
        }
    }
}

// ---------- 渲染 ----------

/// 渲染系统设置页。`submitted` 为 Some 时用它回填（校验失败保留已填值），
/// None 时从 settings 表读取当前值；`error` 展示在对应区块。
async fn render(
    state: &AppState,
    session: &Session,
    path: &str,
    submitted: Option<&HashMap<String, String>>,
    settings_error: &str,
    password_error: &str,
) -> Response {
    let (mut ctx, _csrf) = super::base_ctx(state, session, path).await;
    let values: HashMap<String, String> = match submitted {
        Some(form) => FORM_KEYS
            .iter()
            .map(|k| (k.to_string(), form.get(*k).cloned().unwrap_or_default()))
            .collect(),
        None => {
            let mut v =
                crate::services::settings::get_many(&state.db, &FORM_KEYS).await.unwrap_or_default();
            for k in FORM_KEYS {
                v.entry(k.to_string()).or_default();
            }
            v
        }
    };
    ctx.insert("form", &values);
    ctx.insert("timezones", &TIMEZONES);
    ctx.insert("settings_error", settings_error);
    ctx.insert("password_error", password_error);
    super::render_admin(state, "system.html", &ctx)
}

// ---------- 校验 ----------

/// 校验系统设置表单（仅校验出现的字段）。
fn validate(form: &HashMap<String, String>) -> Vec<String> {
    let mut errors = Vec::new();
    if let Some(theme_mode) = form.get("theme_mode") {
        if !THEME_MODES.contains(&theme_mode.as_str()) {
            errors.push("主题模式不合法".into());
        }
    }
    if let Some(timezone) = form.get("timezone") {
        if chrono_tz::Tz::from_str(timezone.trim()).is_err() {
            errors.push("时区不合法".into());
        }
    }
    if let Some(date_format) = form.get("date_format") {
        if !matches!(date_format.trim(), "datetime" | "date") {
            errors.push("日期格式不合法".into());
        }
    }
    errors
}
