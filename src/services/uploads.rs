//! 附件上传服务：类型白名单、大小上限、UUID 命名、图片压缩与落盘/落库。
//!
//! 落盘路径 `<data>/uploads/{image|video|file}/<uuid>.<ext>`，DB `path` 列存相对路径
//! `{sub}/{uuid}.{ext}`。图片压缩只缩小不放大；GIF 原样保留；`image_compress=false` 跳过。

use crate::config::Config;
use crate::db::Db;
use crate::error::AppError;
use crate::models::{Attachment, AttachmentKind};
use axum::extract::Multipart;
use chrono::{DateTime, Utc};
use image::GenericImageView;
use sqlx::{FromRow, Row};
use std::path::Path;
use std::str::FromStr;
use uuid::Uuid;

/// 白名单：(mime, 扩展名数组)。mime 与扩展名需同时命中才放行。
const IMAGE_TYPES: &[(&str, &[&str])] = &[
    ("image/jpeg", &["jpg", "jpeg"]),
    ("image/png", &["png"]),
    ("image/webp", &["webp"]),
    ("image/gif", &["gif"]),
];
const VIDEO_TYPES: &[(&str, &[&str])] = &[
    ("video/mp4", &["mp4"]),
    ("video/webm", &["webm"]),
    ("video/quicktime", &["mov"]),
];
const FILE_TYPES: &[(&str, &[&str])] = &[
    ("application/pdf", &["pdf"]),
    ("text/plain", &["txt"]),
    ("application/zip", &["zip"]),
    ("application/x-gzip", &["gz"]),
    ("application/octet-stream", &["bin"]),
    ("text/markdown", &["md"]),
];

/// mime + 扩展名双校验，判定附件种类（白名单外返回 BadRequest）。
pub fn detect_kind(mime: &str, ext: &str) -> Result<AttachmentKind, AppError> {
    let ext = ext.trim_start_matches('.').to_lowercase();
    if IMAGE_TYPES
        .iter()
        .any(|(m, exts)| m == &mime && exts.contains(&ext.as_str()))
    {
        return Ok(AttachmentKind::Image);
    }
    if VIDEO_TYPES
        .iter()
        .any(|(m, exts)| m == &mime && exts.contains(&ext.as_str()))
    {
        return Ok(AttachmentKind::Video);
    }
    if FILE_TYPES
        .iter()
        .any(|(m, exts)| m == &mime && exts.contains(&ext.as_str()))
    {
        return Ok(AttachmentKind::File);
    }
    Err(AppError::BadRequest(format!(
        "不支持的文件类型: {mime} / .{ext}"
    )))
}

/// 白名单/大小校验 → UUID 命名 → 图片压缩 → 写盘 → INSERT attachments → 回读返回。
pub async fn save_bytes(
    db: &Db,
    cfg: &Config,
    uploads_dir: &Path,
    orig_name: &str,
    mime: &str,
    data: &[u8],
) -> Result<Attachment, AppError> {
    let ext = Path::new(orig_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let kind = detect_kind(mime, &ext)?;
    let max = match kind {
        AttachmentKind::Image => cfg.upload_max_image,
        AttachmentKind::Video => cfg.upload_max_video,
        AttachmentKind::File => cfg.upload_max_file,
    };
    if data.len() as u64 > max {
        return Err(AppError::BadRequest(format!(
            "文件超过大小上限（{max} 字节）"
        )));
    }
    let uuid_name = format!("{}.{ext}", Uuid::new_v4());
    let sub = match kind {
        AttachmentKind::Image => "image",
        AttachmentKind::Video => "video",
        AttachmentKind::File => "file",
    };
    let dir = uploads_dir.join(sub);
    std::fs::create_dir_all(&dir).map_err(internal)?;
    let stored = if kind == AttachmentKind::Image && mime != "image/gif" && cfg.image_compress {
        compress_image(&dir.join(&uuid_name), data, cfg.image_max_edge, cfg.image_quality)?
    } else {
        data.to_vec()
    };
    std::fs::write(dir.join(&uuid_name), &stored).map_err(internal)?;
    let id = sqlx::query(
        "INSERT INTO attachments(uuid_name,orig_name,mime,size,kind,path) VALUES (?,?,?,?,?,?)",
    )
    .bind(&uuid_name)
    .bind(orig_name)
    .bind(mime)
    .bind(stored.len() as i64)
    .bind(kind.to_str())
    .bind(format!("{sub}/{uuid_name}"))
    .execute(db)
    .await?
    .last_insert_rowid();
    get_attachment(db, id)
        .await?
        .ok_or_else(|| AppError::Internal("附件落库失败".into()))
}

/// 解析 multipart（字段名 `files`），逐个保存：单文件失败跳过并 warn，
/// 至少一个成功才 Ok；全部失败时返回第一个错误（保留原始错误信息）。
pub async fn save_upload_multipart(
    db: &Db,
    cfg: &Config,
    uploads_dir: &Path,
    mut parts: Multipart,
) -> Result<Vec<Attachment>, AppError> {
    let mut saved = Vec::new();
    let mut first_err: Option<AppError> = None;
    while let Some(field) = parts.next_field().await.map_err(internal)? {
        if field.name() != Some("files") {
            continue;
        }
        let Some(file_name) = field.file_name() else {
            continue;
        };
        // file_name 为借用，先转 String 再消费 field（bytes() 会 move）。
        let file_name = file_name.to_string();
        let mime = field.content_type().unwrap_or("").to_string();
        let data = field.bytes().await.map_err(internal)?;
        match save_bytes(db, cfg, uploads_dir, &file_name, &mime, &data).await {
            Ok(att) => saved.push(att),
            Err(e) => {
                if first_err.is_none() {
                    first_err = Some(e);
                }
                tracing::warn!("上传文件 {file_name} 失败");
            }
        }
    }
    if saved.is_empty() {
        return Err(first_err.unwrap_or_else(|| {
            AppError::BadRequest("没有成功上传的文件".into())
        }));
    }
    Ok(saved)
}

/// 图片压缩：最长边缩至 `max_edge`（只缩小不放大），JPEG 用 `quality` 质量重编码，
/// PNG/WebP 保持原格式重编码，GIF 原样返回。
///
/// 解压炸弹防护（I6）：解码像素前先读声明尺寸（PNG IHDR / JPEG 头等，不解码像素），
/// 最长边超 `max_edge * 4` 直接拒绝——高分辨率纯色图体积小但解码会 OOM。
pub fn compress_image(
    path: &Path,
    data: &[u8],
    max_edge: u32,
    quality: u8,
) -> Result<Vec<u8>, AppError> {
    let (w, h) = image::ImageReader::new(std::io::Cursor::new(data))
        .with_guessed_format()
        .map_err(|e| AppError::BadRequest(format!("图片解码失败: {e}")))?
        .into_dimensions()
        .map_err(|e| AppError::BadRequest(format!("图片解码失败: {e}")))?;
    if w.max(h) > max_edge * 4 {
        return Err(AppError::BadRequest("图片尺寸过大".into()));
    }
    let img = image::ImageReader::new(std::io::Cursor::new(data))
        .with_guessed_format()
        .map_err(|e| AppError::BadRequest(format!("图片解码失败: {e}")))?
        .decode()
        .map_err(|e| AppError::BadRequest(format!("图片解码失败: {e}")))?;
    let (w, h) = img.dimensions();
    let scaled = if w.max(h) > max_edge {
        let ratio = max_edge as f32 / w.max(h) as f32;
        img.resize(
            (w as f32 * ratio) as u32,
            (h as f32 * ratio) as u32,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        img
    };
    let fmt = image::ImageFormat::from_path(path).unwrap_or(image::ImageFormat::Jpeg);
    if fmt == image::ImageFormat::Gif {
        return Ok(data.to_vec());
    }
    let mut out = std::io::Cursor::new(Vec::new());
    if fmt == image::ImageFormat::Jpeg {
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
            .encode_image(&scaled)
            .map_err(internal)?;
    } else {
        scaled.write_to(&mut out, fmt).map_err(internal)?;
    }
    Ok(out.into_inner())
}

/// 分页列出附件（kind=None 全量），支持按时间升降序与文件名关键词（q）。
/// 返回（列表, 总数），供后台附件库卡片网格使用。
pub async fn list_attachments(
    db: &Db,
    kind: Option<AttachmentKind>,
    asc: bool,
    q: Option<&str>,
    page: i64,
    page_size: i64,
) -> Result<(Vec<Attachment>, i64), AppError> {
    const COLUMNS: &str = "id, uuid_name, orig_name, mime, size, kind, path, created_at";
    let mut where_sql = String::new();
    let mut binds: Vec<String> = Vec::new();
    if let Some(k) = kind {
        where_sql.push_str(" WHERE kind = ?");
        binds.push(k.to_str().to_string());
    }
    let q = q.map(str::trim).filter(|s| !s.is_empty());
    if let Some(kw) = q {
        if kind.is_some() {
            where_sql.push_str(" AND orig_name LIKE ?");
        } else {
            where_sql.push_str(" WHERE orig_name LIKE ?");
        }
        binds.push(format!("%{kw}%"));
    }

    let count_sql = format!("SELECT COUNT(*) FROM attachments{where_sql}");
    let mut count_q = sqlx::query(&count_sql);
    for b in &binds {
        count_q = count_q.bind(b);
    }
    let total: i64 = count_q.fetch_one(db).await?.get(0);

    let order = if asc { "ASC" } else { "DESC" };
    let item_sql = format!(
        "SELECT {COLUMNS} FROM attachments{where_sql} \
         ORDER BY created_at {order}, id {order} LIMIT ? OFFSET ?"
    );
    let mut q = sqlx::query_as::<_, AttachmentRow>(&item_sql);
    for b in &binds {
        q = q.bind(b);
    }
    q = q.bind(page_size).bind((page - 1) * page_size);
    let rows = q.fetch_all(db).await?;
    let items: Vec<Attachment> = rows.into_iter().map(Into::into).collect();
    Ok((items, total))
}

/// 按 id 读取附件（不存在返回 None）。
pub async fn get_attachment(db: &Db, id: i64) -> Result<Option<Attachment>, AppError> {
    const COLUMNS: &str = "id, uuid_name, orig_name, mime, size, kind, path, created_at";
    let row = sqlx::query_as::<_, AttachmentRow>(&format!(
        "SELECT {COLUMNS} FROM attachments WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(db)
    .await?;
    Ok(row.map(Attachment::from))
}

/// 删除附件：删磁盘文件 + 删 DB 行（不存在视为成功）。
pub async fn delete_attachment(
    db: &Db,
    uploads_dir: &Path,
    id: i64,
) -> Result<(), AppError> {
    let Some(att) = get_attachment(db, id).await? else {
        return Ok(());
    };
    let full = uploads_dir.join(&att.path);
    if full.exists() {
        std::fs::remove_file(&full).map_err(internal)?;
    }
    sqlx::query("DELETE FROM attachments WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

/// 数据库行结构：kind 以 String 存取，经 `to_str`/`from_str` 与模型互转。
#[derive(FromRow)]
struct AttachmentRow {
    id: i64,
    uuid_name: String,
    orig_name: String,
    mime: String,
    size: i64,
    kind: String,
    path: String,
    created_at: DateTime<Utc>,
}

impl From<AttachmentRow> for Attachment {
    fn from(r: AttachmentRow) -> Self {
        Attachment {
            id: r.id,
            uuid_name: r.uuid_name,
            orig_name: r.orig_name,
            mime: r.mime,
            size: r.size,
            kind: AttachmentKind::from_str(&r.kind).unwrap_or_default(),
            path: r.path,
            created_at: r.created_at,
        }
    }
}

fn internal(e: impl std::fmt::Display) -> AppError {
    AppError::Internal(e.to_string())
}
