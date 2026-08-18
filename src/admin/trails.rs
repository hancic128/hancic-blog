//! 后台徒步轨迹管理：上传 GPX（自动解析入库）/ 列表 / 编辑名称描述 / 删除。
//!
//! 上传走 multipart（文件 + 可选名称/描述 + csrf），校验扩展名 `.gpx` 且解析出
//! ≥2 个轨迹点，失败带 `?msg=` 回列表回显；删除同时清理磁盘 GPX 与完整坐标 JSON。

use crate::services::trails;
use crate::{session, AppState};
use axum::extract::{Form, Multipart, OriginalUri, Path, State};
use axum::response::Response;
use serde_json::json;
use std::collections::HashMap;
use tower_sessions::Session;

/// GPX 上传大小上限（两步路导出的 GPX 通常 <5MB，留足余量）。
pub const GPX_MAX_BYTES: usize = 20 * 1024 * 1024;

// ---------- 列表 ----------

pub async fn list(
    State(state): State<AppState>,
    session: Session,
    uri: OriginalUri,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect(&state.config.base_path, "/admin/login");
    }
    let sort_key = sort_param(uri.query().unwrap_or("")).unwrap_or("recent");
    let sort = trails::TrailSort::parse(Some(sort_key));
    let items = trails::list_trails(&state.db, sort).await.unwrap_or_default();
    let (mut ctx, csrf) = super::base_ctx(&state, &session, uri.path()).await;
    ctx.insert("csrf", &csrf);
    ctx.insert("trails", &json!(items.iter().map(trail_admin_value).collect::<Vec<_>>()));
    ctx.insert("sort", &sort_key);
    ctx.insert(
        "error_msg",
        &uri.query()
            .and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("msg=")))
            .map(crate::util::percent_decode)
            .unwrap_or_default(),
    );
    super::render_admin(&state, "trails.html", &ctx)
}

/// 从查询串中取 `sort=` 参数值。
fn sort_param(q: &str) -> Option<&str> {
    q.split('&').find_map(|kv| kv.strip_prefix("sort="))
}

/// 列表项 JSON：原始字段 + 展示用格式化字段（里程 km、运动时长、日期）。
fn trail_admin_value(t: &crate::models::Trail) -> serde_json::Value {
    json!({
        "id": t.id,
        "name": t.name,
        "description": t.description,
        "distance_km": t.distance_m.map(|m| m / 1000.0),
        "distance_km_str": t.distance_m.map(|m| format!("{:.1}", m / 1000.0)),
        "elevation_gain_m": t.elevation_gain_m,
        "elevation_gain_str": t.elevation_gain_m.map(|e| format!("{e:.0}")),
        "point_count": t.point_count,
        "started_at": t.started_at.map(|d| d.date_naive().to_string()),
        "moving_time": trails::format_moving(t.moving_seconds),
    })
}

// ---------- 上传（multipart，支持多选） ----------

/// 单次上传的文件数上限（与路由 body 上限配套）。
pub const MAX_TRAIL_FILES: usize = 10;

pub async fn upload(
    State(state): State<AppState>,
    session: Session,
    multipart: Multipart,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect(&state.config.base_path, "/admin/login");
    }
    let mut csrf_ok = false;
    let mut name = String::new();
    let mut description = String::new();
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut parts = multipart;
    loop {
        let field = match parts.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => {
                tracing::warn!("解析上传 multipart 失败: {e}");
                break;
            }
        };
        match field.name() {
            Some("csrf") => {
                let value = field.text().await.unwrap_or_default();
                csrf_ok = session::verify_csrf(&session, Some(&value)).await.is_ok();
            }
            Some("name") => name = field.text().await.unwrap_or_default(),
            Some("description") => description = field.text().await.unwrap_or_default(),
            Some("file") => {
                let file_name = field.file_name().map(str::to_string).unwrap_or_default();
                let data = field.bytes().await.unwrap_or_default();
                files.push((file_name, data.to_vec()));
            }
            _ => {}
        }
    }
    if !csrf_ok {
        return fail(&state.config.base_path, "安全校验失败，请刷新页面后重试");
    }
    if files.is_empty() {
        return fail(&state.config.base_path, "请选择 GPX 文件");
    }
    if files.len() > MAX_TRAIL_FILES {
        return fail(
            &state.config.base_path,
            &format!("单次最多上传 {MAX_TRAIL_FILES} 个文件"),
        );
    }

    // 逐个导入：汇总成功/失败，失败不中断其余文件
    let mut ok = 0usize;
    let mut errors: Vec<String> = Vec::new();
    for (file_name, data) in files {
        if !file_name.to_lowercase().ends_with(".gpx") {
            errors.push(format!("{file_name}：仅支持 .gpx 文件"));
            continue;
        }
        if data.is_empty() {
            errors.push(format!("{file_name}：文件内容为空"));
            continue;
        }
        // 两步路导出 GPX 无 <name>：默认名称回退到上传文件名（去 .gpx）
        let fallback_name = file_name
            .strip_suffix(".gpx")
            .or_else(|| file_name.strip_suffix(".GPX"))
            .unwrap_or(&file_name)
            .to_string();
        let trails_dir = state.config.data_dir.join("trails");
        match trails::import_gpx(
            &state.db,
            &trails_dir,
            &name,
            &fallback_name,
            &description,
            &data,
        )
        .await
        {
            Ok(_) => ok += 1,
            Err(e) => errors.push(format!("{file_name}：{}", e.message())),
        }
    }

    let base = &state.config.base_path;
    let msg = if ok == 0 {
        format!("导入失败：{}", errors.join("；"))
    } else if errors.is_empty() {
        format!("成功导入 {ok} 条轨迹")
    } else {
        format!("成功导入 {ok} 条；失败 {} 条：{}", errors.len(), errors.join("；"))
    };
    super::redirect(base, &format!("/admin/trails?msg={}", urlencode(&msg)))
}

// ---------- 编辑（名称/描述） ----------

pub async fn update(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<i64>,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect(&state.config.base_path, "/admin/login");
    }
    if session::verify_csrf(&session, form.get("csrf").map(String::as_str))
        .await
        .is_err()
    {
        return fail(&state.config.base_path, "安全校验失败，请刷新页面后重试");
    }
    let name = form.get("name").cloned().unwrap_or_default();
    let description = form.get("description").cloned().unwrap_or_default();
    if let Some(msg) = validate(&name, &description) {
        return fail(&state.config.base_path, msg);
    }
    match trails::update_trail(&state.db, id, name.trim(), &description).await {
        Ok(_) => super::redirect(&state.config.base_path, "/admin/trails"),
        Err(e) => {
            tracing::error!("更新轨迹失败 id={id}: {e:?}");
            fail(&state.config.base_path, e.message())
        }
    }
}

// ---------- 删除 ----------

pub async fn delete(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<i64>,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    if session::require_admin(&session).await.is_err() {
        return super::redirect(&state.config.base_path, "/admin/login");
    }
    if session::verify_csrf(&session, form.get("csrf").map(String::as_str))
        .await
        .is_err()
    {
        return fail(&state.config.base_path, "安全校验失败，请刷新页面后重试");
    }
    let trails_dir = state.config.data_dir.join("trails");
    match trails::delete_trail(&state.db, &trails_dir, id).await {
        Ok(()) => super::redirect(&state.config.base_path, "/admin/trails"),
        Err(e) => {
            tracing::error!("删除轨迹失败 id={id}: {e:?}");
            fail(&state.config.base_path, "删除失败，请重试")
        }
    }
}

// ---------- 辅助 ----------

fn validate(name: &str, description: &str) -> Option<&'static str> {
    if name.trim().is_empty() {
        return Some("轨迹名称不能为空");
    }
    if name.trim().chars().count() > 100 {
        return Some("轨迹名称最多 100 字");
    }
    if description.chars().count() > 500 {
        return Some("轨迹描述最多 500 字");
    }
    None
}

/// 302 回列表并带 URL 编码的错误提示。
fn fail(base: &str, msg: &str) -> Response {
    super::redirect(base, &format!("/admin/trails?msg={}", urlencode(msg)))
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
