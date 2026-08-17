//! 后台站点设置：站点信息保存 + 修改密码。
//!
//! 鉴权约定同其他后台模块：GET 未登录 302 跳登录；POST 先 `require_admin`
//! 再过 CSRF。保存失败不重定向，直接以 200 重渲染设置页并回填用户已填值
//! （`?msg=` 跳转无法携带 JSON/文本域等字段，故校验失败走同页渲染）；
//! 修改密码失败同样同页回显，密码输入框不回填（避免明文回显）。
//!
//! 保存逻辑：`settings::set` 逐项写库，前台 `site_context` 每次请求重读
//! settings 表，保存后即时生效（T7）。

use crate::models::{PostStatus, PostType};
use crate::services::{posts, settings};
use crate::{session, AppState};
use axum::extract::{Form, OriginalUri, State};
use axum::response::Response;
use serde_json::{json, Value};
use std::collections::HashMap;
use tower_sessions::Session;

/// 设置表单字段（settings 表键名，与前台 `site_context` 读取一致）。
/// 站点信息 + 页脚/友情链接 + 悬浮联系方式卡片。
const FORM_KEYS: [&str; 10] = [
    "site_name",
    "site_desc",
    "site_nav",
    "site_social",
    "social_logos",
    "site_logo",
    "footer_text",
    "friend_links",
    "contact_enabled",
    "contact_email",
];

// ---------- 设置页 ----------

pub async fn page(State(state): State<AppState>, session: Session, uri: OriginalUri) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect(&state.config.base_path, "/admin/login");
    }
    let saved = uri
        .query()
        .is_some_and(|q| q.split('&').any(|kv| kv == "saved=1"));
    render(&state, &session, uri.path(), None, "", saved).await
}

// ---------- 保存站点信息 ----------

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
            false,
        )
        .await;
    }
    // 校验失败：错误收集后同页回显（保留已填值）
    let errors = validate(&form);
    if !errors.is_empty() {
        let msg = errors.join("；");
        return render(&state, &session, uri.path(), Some(&form), &msg, false).await;
    }
    // 逐项写库（仅更新表单中出现的字段——设置页拆为多个独立表单，
    // 各自提交自己的字段，缺失键保持库中原值，避免误清空）
    for key in FORM_KEYS {
        if let Some(value) = form.get(key) {
            if let Err(e) = settings::set(&state.db, key, value.trim()).await {
                tracing::error!("保存设置 {key} 失败: {e:?}");
                return render(
                    &state,
                    &session,
                    uri.path(),
                    Some(&form),
                    "保存失败，请重试",
                    false,
                )
                .await;
            }
        }
    }
    // 成功：跳回设置页并带 saved 标记，页面弹「保存成功」提示
    super::redirect(&state.config.base_path, "/admin/settings?saved=1")
}

// ---------- 渲染 ----------

/// 渲染设置页。
///
/// `submitted` 为 Some 时用它回填站点信息表单（校验失败保留已填值），
/// None 时从 settings 表读取当前值；`settings_error` 展示在站点信息区块；
/// `saved` 为 true 时（重定向带 ?saved=1）页面弹「保存成功」轻提示。
async fn render(
    state: &AppState,
    session: &Session,
    path: &str,
    submitted: Option<&HashMap<String, String>>,
    settings_error: &str,
    saved: bool,
) -> Response {
    let (mut ctx, _csrf) = super::base_ctx(state, session, path).await;
    let values: HashMap<String, String> = match submitted {
        Some(form) => FORM_KEYS
            .iter()
            .map(|k| (k.to_string(), form.get(*k).cloned().unwrap_or_default()))
            .collect(),
        None => {
            // get_many 只返回数据库存在的键；缺失键补空值，避免模板访问未定义字段
            let mut v = settings::get_many(&state.db, &FORM_KEYS).await.unwrap_or_default();
            for k in FORM_KEYS {
                v.entry(k.to_string()).or_default();
            }
            v
        }
    };
    ctx.insert("form", &values);
    ctx.insert("settings_error", settings_error);
    ctx.insert("saved", &saved);
    // 全部已发布独立页（type=page），供导航「页面」类型搜索选择
    let (all_posts, _) = posts::list_posts(
        &state.db,
        posts::PostListOptions {
            status: Some(PostStatus::Published),
            post_type: Some(PostType::Page),
            category_slug: None,
            tag_slug: None,
            column_slug: None,
            month: None,
            sort: None,
            page: 1,
            page_size: 1000,
        },
    )
    .await
    .unwrap_or_default();
    let all_posts_json = serde_json::to_string(&all_posts.iter().map(|p| {
        json!({ "slug": p.slug, "title": p.title, "type": p.post_type.to_str() })
    }).collect::<Vec<_>>())
    .unwrap_or_else(|_| "[]".into());
    ctx.insert("all_posts_json", &all_posts_json);
    super::render_admin(state, "settings.html", &ctx)
}

// ---------- 校验 ----------

/// 校验设置表单，返回错误列表（空表示全部通过）。
/// 仅校验表单中出现的字段（设置页拆为多个独立表单，各自提交部分字段）。
fn validate(form: &HashMap<String, String>) -> Vec<String> {
    let mut errors = Vec::new();
    if let Some(site_name) = form.get("site_name") {
        if site_name.trim().is_empty() {
            errors.push("站点名称不能为空".into());
        }
    }
    if let Some(site_nav) = form.get("site_nav") {
        if let Some(msg) = validate_nav(site_nav) {
            errors.push(msg);
        }
    }
    if let Some(site_social) = form.get("site_social") {
        if let Some(msg) = validate_social(site_social) {
            errors.push(msg);
        }
    }
    // 空串跳过：未填写的可选字段（友情链接/二维码/社交图标）不校验
    if let Some(friend_links) = form.get("friend_links") {
        if !friend_links.trim().is_empty() {
            if let Some(msg) = validate_nav(friend_links) {
                errors.push(format!("友情链接：{msg}"));
            }
        }
    }
    if let Some(contact_enabled) = form.get("contact_enabled") {
        if !contact_enabled.is_empty()
            && contact_enabled != "1"
            && contact_enabled != "0"
        {
            errors.push("联系方式开关不合法".into());
        }
    }
    if let Some(social_logos) = form.get("social_logos") {
        if !social_logos.trim().is_empty() {
            if let Some(msg) = validate_social(social_logos) {
                errors.push(format!("社交图标：{msg}"));
            }
        }
    }
    errors
}

/// 校验导航 JSON：必须为数组，每项含非空 label，type 合法
/// （home/articles/moments/pages/column/link，缺省 link）；仅 link 类型必须填 url
/// （首页/文章/说说路径由类型预设，页面/专栏为下拉入口无需路径）。
fn validate_nav(s: &str) -> Option<String> {
    let v: Value = match serde_json::from_str(s) {
        Ok(v) => v,
        Err(_) => return Some("导航必须为 JSON 数组".into()),
    };
    let Some(arr) = v.as_array() else {
        return Some("导航必须为 JSON 数组".into());
    };
    const TYPES: [&str; 7] = ["home", "articles", "moments", "pages", "column", "trail", "link"];
    for (i, item) in arr.iter().enumerate() {
        let label = item.get("label").and_then(Value::as_str).unwrap_or("").trim();
        let url = item.get("url").and_then(Value::as_str).unwrap_or("").trim();
        let ty = item.get("type").and_then(Value::as_str).unwrap_or("link");
        if !TYPES.contains(&ty) {
            return Some(format!("导航第 {} 项的类型不合法", i + 1));
        }
        if label.is_empty() {
            return Some(format!("导航第 {} 项缺少名称", i + 1));
        }
        // 自定义链接必须有跳转地址；其余类型路径由预设/下拉决定
        if ty == "link" && url.is_empty() {
            return Some(format!("导航第 {} 项缺少链接", i + 1));
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
