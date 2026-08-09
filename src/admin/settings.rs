//! 后台站点设置：站点信息保存 + 修改密码。
//!
//! 鉴权约定同其他后台模块：GET 未登录 302 跳登录；POST 先 `require_admin`
//! 再过 CSRF。保存失败不重定向，直接以 200 重渲染设置页并回填用户已填值
//! （`?msg=` 跳转无法携带 JSON/文本域等字段，故校验失败走同页渲染）；
//! 修改密码失败同样同页回显，密码输入框不回填（避免明文回显）。
//!
//! 保存逻辑：`settings::set` 逐项写库，前台 `site_context` 每次请求重读
//! settings 表，保存后即时生效（T7）。

use crate::services::settings;
use crate::{auth, session, AppState};
use axum::extract::{Form, OriginalUri, State};
use axum::response::Response;
use serde_json::Value;
use std::collections::HashMap;
use std::str::FromStr;
use tower_sessions::Session;

/// 设置表单字段（settings 表键名，与前台 `site_context` 读取一致）。
const FORM_KEYS: [&str; 6] = [
    "site_name",
    "site_desc",
    "site_nav",
    "site_social",
    "theme_mode",
    "timezone",
];

/// 允许的主题模式。
const THEME_MODES: [&str; 3] = ["auto", "light", "dark"];

// ---------- 设置页 ----------

pub async fn page(State(state): State<AppState>, session: Session, uri: OriginalUri) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect("/admin/login");
    }
    render(&state, &session, uri.path(), None, "", "").await
}

// ---------- 保存站点信息 ----------

pub async fn save(
    State(state): State<AppState>,
    session: Session,
    uri: OriginalUri,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect("/admin/login");
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
    // 校验失败：错误收集后同页回显（保留已填值）
    let errors = validate(&form);
    if !errors.is_empty() {
        let msg = errors.join("；");
        return render(&state, &session, uri.path(), Some(&form), &msg, "").await;
    }
    // 逐项写库（trim 后存储；校验已保证非空与格式合法）
    for key in FORM_KEYS {
        let value = form.get(key).map(String::as_str).unwrap_or("").trim();
        if let Err(e) = settings::set(&state.db, key, value).await {
            tracing::error!("保存设置 {key} 失败: {e:?}");
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
    super::redirect("/admin/settings")
}

// ---------- 修改密码 ----------

pub async fn password(
    State(state): State<AppState>,
    session: Session,
    uri: OriginalUri,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect("/admin/login");
    }
    if session::verify_csrf(&session, form.get("csrf").map(String::as_str))
        .await
        .is_err()
    {
        return render(&state, &session, uri.path(), None, "", "安全校验失败，请刷新页面后重试")
            .await;
    }
    let old = form.get("old_password").map(String::as_str).unwrap_or("");
    let new = form.get("new_password").map(String::as_str).unwrap_or("");
    let confirm = form.get("confirm").map(String::as_str).unwrap_or("");

    // 旧密码校验：argon2 为 CPU 密集操作，走 spawn_blocking（与登录一致）
    let Some(hash) = auth::get_password_hash(&state.db).await.ok().flatten() else {
        return render(
            &state,
            &session,
            uri.path(),
            None,
            "",
            "尚未设置管理员密码，请通过安装流程初始化",
        )
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
    if new.chars().count() < auth::PASSWORD_MIN_LEN {
        let msg = format!("新密码至少 {} 个字符", auth::PASSWORD_MIN_LEN);
        return render(&state, &session, uri.path(), None, "", &msg).await;
    }
    if new_same_as_old {
        return render(&state, &session, uri.path(), None, "", "新密码不能与旧密码相同").await;
    }
    if new != confirm {
        return render(&state, &session, uri.path(), None, "", "两次输入的新密码不一致").await;
    }
    match auth::set_password(&state.db, new).await {
        Ok(()) => super::redirect("/admin/settings"),
        Err(e) => {
            tracing::error!("修改密码失败: {e:?}");
            render(&state, &session, uri.path(), None, "", "密码修改失败，请重试").await
        }
    }
}

// ---------- 渲染 ----------

/// 渲染设置页。
///
/// `submitted` 为 Some 时用它回填站点信息表单（校验失败保留已填值），
/// None 时从 settings 表读取当前值；`settings_error` / `password_error`
/// 分别展示在站点信息与修改密码区块。
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
        None => settings::get_many(&state.db, &FORM_KEYS).await.unwrap_or_default(),
    };
    ctx.insert("form", &values);
    ctx.insert("settings_error", settings_error);
    ctx.insert("password_error", password_error);
    super::render_admin(state, "settings.html", &ctx)
}

// ---------- 校验 ----------

/// 校验设置表单，返回错误列表（空表示全部通过）。
fn validate(form: &HashMap<String, String>) -> Vec<String> {
    let mut errors = Vec::new();
    let site_name = form.get("site_name").map(String::as_str).unwrap_or("");
    if site_name.trim().is_empty() {
        errors.push("站点名称不能为空".into());
    }
    if let Some(msg) = validate_nav(form.get("site_nav").map(String::as_str).unwrap_or("")) {
        errors.push(msg);
    }
    if let Some(msg) = validate_social(form.get("site_social").map(String::as_str).unwrap_or("")) {
        errors.push(msg);
    }
    let theme_mode = form.get("theme_mode").map(String::as_str).unwrap_or("");
    if !THEME_MODES.contains(&theme_mode) {
        errors.push("主题模式不合法".into());
    }
    let timezone = form.get("timezone").map(String::as_str).unwrap_or("");
    if chrono_tz::Tz::from_str(timezone.trim()).is_err() {
        errors.push("时区不合法".into());
    }
    errors
}

/// 校验导航 JSON：必须为数组，每项含非空 label/url。
fn validate_nav(s: &str) -> Option<String> {
    let v: Value = match serde_json::from_str(s) {
        Ok(v) => v,
        Err(_) => return Some("导航必须为 JSON 数组".into()),
    };
    let Some(arr) = v.as_array() else {
        return Some("导航必须为 JSON 数组".into());
    };
    for (i, item) in arr.iter().enumerate() {
        let label = item.get("label").and_then(Value::as_str).unwrap_or("").trim();
        let url = item.get("url").and_then(Value::as_str).unwrap_or("").trim();
        if label.is_empty() || url.is_empty() {
            return Some(format!("导航第 {} 项缺少 label 或 url", i + 1));
        }
    }
    None
}

/// 校验社交链接 JSON：必须为对象，各键值均为字符串。
fn validate_social(s: &str) -> Option<String> {
    let v: Value = match serde_json::from_str(s) {
        Ok(v) => v,
        Err(_) => return Some("社交链接必须为 JSON 对象".into()),
    };
    let Some(obj) = v.as_object() else {
        return Some("社交链接必须为 JSON 对象".into());
    };
    for (k, val) in obj {
        if !val.is_string() {
            return Some(format!("社交链接 {k} 的值必须是字符串"));
        }
    }
    None
}
