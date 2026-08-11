//! 后台主题管理：已装主题列表、切换与预览。
//!
//! 鉴权约定同其他后台模块：GET 未登录 302 跳登录；POST 先 `require_admin`
//! 再过 CSRF。activate 写 settings.active_theme——前台 `site_context` 每次
//! 请求重读该键，站点信息层即时生效；前台模板渲染器 `AppState.tera` 在
//! 启动时按 `settings.active_theme`（优先于 `config.active_theme`）构建
//! （见 lib.rs），故切主题=写 DB，重启后模板/样式完全切换。
//! preview 302 到 `/?theme_preview=`，前台按预览主题名临时构建 tera 渲染
//! （只读覆盖，不落库，见 front.rs）。

use crate::services::settings;
use crate::themes;
use crate::{session, AppState};
use axum::extract::{Form, Multipart, OriginalUri, Path, Query, State};
use axum::response::Response;
use serde_json::{Value, json};
use std::collections::HashMap;
use tower_sessions::Session;

/// 主题 zip 上传大小上限（100MB，主题包通常远小于此）。
const MAX_THEME_ZIP_BYTES: usize = 100 * 1024 * 1024;

// ---------- 列表 ----------

pub async fn list(
    State(state): State<AppState>,
    session: Session,
    Query(query): Query<HashMap<String, String>>,
    uri: OriginalUri,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect(&state.config.base_path,  "/admin/login");
    }
    // discover 实时扫描主题目录：复制新主题进 data_dir 后无需重启即可见
    let themes_dir = state.config.data_dir.join("themes");
    let metas = match themes::discover(&themes_dir) {
        Ok(metas) => metas,
        Err(e) => {
            tracing::error!("扫描主题目录失败: {e}");
            Vec::new()
        }
    };
    // 当前主题以 settings 为准（激活后 site_context 即时生效），缺省回退配置默认值
    let current = settings::get(&state.db, "active_theme")
        .await
        .ok()
        .flatten()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| state.config.active_theme.clone());
    let (mut ctx, _csrf) = super::base_ctx(&state, &session, uri.path()).await;
    ctx.insert("themes", &themes_value(&metas, &current));
    ctx.insert("current_theme", &current);
    // 切换结果提示（`?msg=`，见 `redirect_msg`）
    ctx.insert(
        "msg",
        &query.get("msg").map(String::as_str).unwrap_or(""),
    );
    super::render_admin(&state, "themes.html", &ctx)
}

/// 主题列表 JSON：名称/作者/版本/描述 + 是否当前（模板据此展示徽标与操作）。
fn themes_value(metas: &[themes::ThemeMeta], current: &str) -> Value {
    json!(metas
        .iter()
        .map(|m| json!({
            "name": m.name,
            "author": m.author,
            "version": m.version,
            "description": m.description,
            "is_current": m.name == current,
        }))
        .collect::<Vec<_>>())
}

// ---------- 切换 ----------

pub async fn activate(
    State(state): State<AppState>,
    session: Session,
    Path(name): Path<String>,
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
    // 主题必须真实存在（目录 + theme.toml），防止写入不存在的名字
    let themes_dir = state.config.data_dir.join("themes");
    if !themes::is_valid_name(&name) || themes::load_meta(&themes_dir, &name).is_err() {
        return redirect_msg(&state.config.base_path, "主题不存在");
    }
    if let Err(e) = settings::set(&state.db, "active_theme", &name).await {
        tracing::error!("切换主题 {name} 失败: {e:?}");
        return redirect_msg(&state.config.base_path, "切换失败，请重试");
    }
    // 前台模板渲染器在启动时按 DB 的 active_theme 构建：模板/样式需重启才切换，
    // 重启后以 DB 为准（C2），故提示重启生效
    redirect_msg(&state.config.base_path, &format!("已切换到 {name}，重启服务后完全生效"))
}

// ---------- 预览 ----------

/// 302 到前台首页并携带 `?theme_preview=`：前台以只读覆盖渲染预览主题，
/// 不修改 settings（见 web::front）。
pub async fn preview(
    State(state): State<AppState>,
    session: Session,
    Path(name): Path<String>,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect(&state.config.base_path,  "/admin/login");
    }
    let themes_dir = state.config.data_dir.join("themes");
    if !themes::is_valid_name(&name) || themes::load_meta(&themes_dir, &name).is_err() {
        return redirect_msg(&state.config.base_path, "主题不存在");
    }
    super::redirect(&state.config.base_path, &format!("/?theme_preview={name}"))
}

// ---------- 导入 / 卸载 ----------

/// 导入主题：multipart（`csrf` + `theme` 文件字段），安装成功提示（需重启生效）。
pub async fn import(
    State(state): State<AppState>,
    session: Session,
    mut multipart: Multipart,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect(&state.config.base_path,  "/admin/login");
    }
    let mut csrf = None;
    let mut zip_bytes: Option<Vec<u8>> = None;
    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if name == "csrf" {
            csrf = field.text().await.ok();
        } else if name == "theme" {
            zip_bytes = field.bytes().await.ok().map(|b| b.to_vec());
        }
    }
    let Some(csrf) = csrf else {
        return redirect_msg(&state.config.base_path, "安全校验失败，请刷新页面后重试");
    };
    if session::verify_csrf(&session, Some(&csrf)).await.is_err() {
        return redirect_msg(&state.config.base_path, "安全校验失败，请刷新页面后重试");
    }
    let Some(bytes) = zip_bytes else {
        return redirect_msg(&state.config.base_path, "请选择主题 zip 文件");
    };
    if bytes.is_empty() {
        return redirect_msg(&state.config.base_path, "主题包为空");
    }
    if bytes.len() > MAX_THEME_ZIP_BYTES {
        return redirect_msg(&state.config.base_path, "主题包超过 100MB 上限");
    }
    let themes_dir = state.config.data_dir.join("themes");
    match themes::install(&themes_dir, &bytes) {
        Ok(meta) => {
            tracing::info!("主题导入成功: {} v{}", meta.name, meta.version);
            redirect_msg(
                &state.config.base_path,
                &format!("主题「{}」导入成功，重启服务后完全生效", meta.name),
            )
        }
        Err(e) => redirect_msg(&state.config.base_path, &format!("导入失败: {e}")),
    }
}

/// 卸载主题：仅允许删除非当前激活主题（默认主题 data 副本允许删除，
/// 但当前使用中会拒绝）。
pub async fn uninstall(
    State(state): State<AppState>,
    session: Session,
    Path(name): Path<String>,
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
    let themes_dir = state.config.data_dir.join("themes");
    if !themes::is_valid_name(&name) || themes::load_meta(&themes_dir, &name).is_err() {
        return redirect_msg(&state.config.base_path, "主题不存在");
    }
    // 当前激活主题不可卸载（前台依赖其模板/静态资源）
    let current = settings::get(&state.db, "active_theme")
        .await
        .ok()
        .flatten()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| state.config.active_theme.clone());
    if current == name {
        return redirect_msg(&state.config.base_path, "不能卸载当前使用的主题");
    }
    let dir = themes_dir.join(&name);
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => {
            tracing::info!("主题卸载: {name}");
            redirect_msg(&state.config.base_path, &format!("主题「{name}」已卸载"))
        }
        Err(e) => {
            tracing::error!("卸载主题 {name} 失败: {e:?}");
            redirect_msg(&state.config.base_path, "卸载失败，请重试")
        }
    }
}

// ---------- 工具 ----------

/// 带 `?msg=` 查询参数的重定向（操作结果提示，成功与失败均走此通道）。
fn redirect_msg(base: &str, msg: &str) -> Response {
    super::redirect(base, &format!("/admin/themes?msg={}", urlencode(msg)))
}

/// 查询参数值百分号编码（RFC 3986：仅保留 unreserved 字符；与 taxonomy 同款）。
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn themes_value_marks_current() {
        let metas = vec![
            themes::ThemeMeta {
                name: "default".into(),
                author: "hancic".into(),
                version: "0.1.0".into(),
                description: String::new(),
            },
            themes::ThemeMeta {
                name: "test-theme".into(),
                author: "hancic".into(),
                version: "0.0.1".into(),
                description: String::new(),
            },
        ];
        let v = themes_value(&metas, "test-theme");
        let arr = v.as_array().unwrap();
        assert_eq!(arr[0]["is_current"], false);
        assert_eq!(arr[1]["is_current"], true);
        assert_eq!(arr[1]["name"], "test-theme");
    }
}
