//! Halo Markdown zip 迁移导入服务。
//!
//! 遍历 zip 内任意层级的 `*.md`：front-matter（手写解析 `---` 块，字段
//! title/date/categories/tags/slug/type）→ 分类/标签 `ensure` → `create_post`
//! （同 slug 查重跳过，不再自动后缀重建，I7；status=published；published_at 取
//! front-matter date）→
//! 正文图片管线（`![alt](url)` 收集）：zip 内相对路径图片解包走 `save_bytes`
//! 落库，外部 http(s) 图片在 `download_images=true` 时用 reqwest 下载（30s
//! 超时）；成功替换正文 URL 为 `/uploads/<path>`，失败记入报告不中断导入。
//! 无 front-matter 时 title=正文首行 `# `，slug=文件名；空文件跳过。

use crate::config::Config;
use crate::db::Db;
use crate::error::AppError;
use crate::models::{PostStatus, PostType};
use crate::services::{posts, taxonomy, uploads};
use super::{MAX_ENTRY_BYTES, MAX_TOTAL_BYTES};
use chrono::{DateTime, NaiveDate, NaiveDateTime, SecondsFormat, Utc};
use serde::Serialize;

use std::io::Read;
use std::path::Path;
use std::time::Duration;

/// 迁移导入报告：计数 + 失败明细（单篇/图片失败不中断整体导入）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct ImportReport {
    pub posts_created: usize,
    pub posts_skipped: usize,
    pub categories: usize,
    pub tags: usize,
    pub images_downloaded: usize,
    pub images_failed: usize,
    pub failures: Vec<String>,
}

/// front-matter 解析结果：title 为 String（缺省空串，无 front-matter 时由调用方
/// 取正文首行标题），date 原样保留（落库时再解析）。
#[derive(Debug, Clone, Default)]
pub struct FrontMatter {
    pub title: String,
    pub date: Option<String>,
    pub categories: Vec<String>,
    pub tags: Vec<String>,
    pub slug: Option<String>,
    pub post_type: Option<String>,
}

/// 解析 `---\n...\n---` front-matter 块（值支持 `[a, b]` 列表与引号）。
/// 无 front-matter（首行不是 `---`）返回空 FrontMatter；块未闭合视为解析错误。
pub fn parse_front_matter(raw: &str) -> Result<FrontMatter, String> {
    split_front_matter(raw).map(|(fm, _)| fm)
}

/// 拆分 front-matter 与正文。无 front-matter 时正文即全文。
fn split_front_matter(raw: &str) -> Result<(FrontMatter, String), String> {
    let mut lines = raw.lines();
    let Some(first) = lines.next() else {
        return Ok((FrontMatter::default(), raw.to_string()));
    };
    if first.trim() != "---" {
        return Ok((FrontMatter::default(), raw.to_string()));
    }
    let mut fm = FrontMatter::default();
    let mut in_front = true;
    let mut closed = false;
    let mut body = String::new();
    for line in lines {
        if in_front && line.trim() == "---" {
            in_front = false;
            closed = true;
            continue;
        }
        if in_front {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim();
            match key.trim().to_lowercase().as_str() {
                "title" => fm.title = parse_scalar(value).unwrap_or_default(),
                "date" => fm.date = parse_scalar(value),
                "slug" => fm.slug = parse_scalar(value),
                "type" => fm.post_type = parse_scalar(value),
                "categories" => fm.categories = parse_list(value),
                "tags" => fm.tags = parse_list(value),
                _ => {}
            }
        } else {
            body.push_str(line);
            body.push('\n');
        }
    }
    if !closed {
        return Err("front-matter 未闭合（缺少结束 ---）".into());
    }
    Ok((fm, body))
}

/// 标量值：去掉首尾引号。
fn parse_scalar(v: &str) -> Option<String> {
    let v = v.trim();
    let v = if (v.starts_with('"') && v.ends_with('"')) || (v.starts_with('\'') && v.ends_with('\''))
    {
        &v[1..v.len() - 1]
    } else {
        v
    };
    let v = v.trim();
    if v.is_empty() {
        None
    } else {
        Some(v.to_string())
    }
}

/// 列表值：`[a, b]` 逗号分隔（支持空列表）；非列表按单个元素处理。
fn parse_list(v: &str) -> Vec<String> {
    let v = v.trim();
    let inner = v
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(v);
    inner
        .split(',')
        .filter_map(parse_scalar)
        .collect()
}

/// 遍历 zip 导入全部 Markdown 文章（含图片落库与正文 URL 替换）。
pub async fn import_halo_zip(
    db: &Db,
    data_dir: &Path,
    zip_path: &Path,
    download_images: bool,
) -> Result<ImportReport, AppError> {
    // save_bytes 需要 Config（大小上限/压缩开关）：从数据目录加载部署配置，
    // 缺失时用默认值（测试场景即如此）。
    let cfg = Config::load(&data_dir.join("config.toml")).map_err(internal)?;
    let uploads_dir = data_dir.join("uploads");
    let file = std::fs::File::open(zip_path).map_err(internal)?;
    let mut archive = zip::ZipArchive::new(file).map_err(internal)?;

    // 第一遍：收集 md 条目索引与图片条目（归一化路径 → 索引）
    let mut md_entries: Vec<usize> = Vec::new();
    let mut image_paths: Vec<(String, usize)> = Vec::new();
    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(internal)?;
        if entry.is_dir() {
            continue;
        }
        let norm = normalize_path(entry.name());
        if norm.is_empty() {
            continue;
        }
        if norm.ends_with(".md") {
            md_entries.push(i);
        } else {
            image_paths.push((norm, i));
        }
    }

    let mut report = ImportReport::default();
    // 外部图片下载复用同一个 reqwest Client（30s 超时），避免每张图重建连接池。
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(internal)?;
    let img_ctx = ImageCtx {
        db,
        cfg: &cfg,
        uploads_dir: &uploads_dir,
        image_paths: &image_paths,
        download_images,
        client: &client,
    };
    // 解压炸弹防护（I8）：md 条目与图片解包均按单条目 500MB / 总量 2GB 限流。
    let mut total_bytes: u64 = 0;
    'posts: for idx in md_entries {
        let mut raw = String::new();
        // 块作用域收束 ZipFile 借用（其 Drop 会让 NLL 保持 archive 可变借用存活）
        let file_name = {
            let mut entry = archive.by_index(idx).map_err(internal)?;
            let file_name = entry.name().to_string();
            let n = match entry.by_ref().take(MAX_ENTRY_BYTES + 1).read_to_string(&mut raw) {
                Ok(n) => n,
                Err(_) => {
                    report.failures.push(format!("{file_name}: 非 UTF-8 编码，跳过"));
                    report.posts_skipped += 1;
                    continue;
                }
            };
            if n as u64 > MAX_ENTRY_BYTES {
                report
                    .failures
                    .push(format!("{file_name}: 单文件超过 500MB，跳过"));
                report.posts_skipped += 1;
                continue;
            }
            total_bytes += n as u64;
            if total_bytes > MAX_TOTAL_BYTES {
                return Err(AppError::BadRequest(
                    "迁移包解压总量超过 2GB 上限，已中止".into(),
                ));
            }
            file_name
        };
        if raw.trim().is_empty() {
            report.posts_skipped += 1;
            continue;
        }
        let (fm, body) = match split_front_matter(&raw) {
            Ok(v) => v,
            Err(e) => {
                report.failures.push(format!("{file_name}: {e}"));
                report.posts_skipped += 1;
                continue;
            }
        };
        if body.trim().is_empty() {
            report.posts_skipped += 1;
            continue;
        }

        // 标题：front-matter title → 正文首行 `# ` → 文件名；slug：front-matter slug → 文件名
        let title = if fm.title.trim().is_empty() {
            first_heading(&body).unwrap_or_else(|| file_stem(&file_name))
        } else {
            fm.title.clone()
        };
        let slug = fm
            .slug
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| file_stem(&file_name));

        // 同 slug 已存在 → 跳过（I7：不再自动加后缀重建，避免重复导入产生重复文章）
        let slug = posts::slugify(&slug).await;
        if posts::get_post_by_slug(db, &slug).await?.is_some() {
            report.posts_skipped += 1;
            continue;
        }

        // 分类：第一个分类作为文章分类；标签全部 ensure。单篇 DB 错误记录
        // failures 并跳过该文件，不中断整个导入。
        let mut category_id = None;
        for name in &fm.categories {
            match ensure_category(db, name).await {
                Ok(c) => {
                    report.categories += 1;
                    if category_id.is_none() {
                        category_id = Some(c.id);
                    }
                }
                Err(e) => {
                    report.failures.push(format!(
                        "{file_name}: 分类 {name} 处理失败 {}",
                        e.message()
                    ));
                    continue 'posts;
                }
            }
        }
        for name in &fm.tags {
            match taxonomy::ensure_tag(db, name).await {
                Ok(_) => report.tags += 1,
                Err(e) => {
                    report.failures.push(format!(
                        "{file_name}: 标签 {name} 处理失败 {}",
                        e.message()
                    ));
                    continue 'posts;
                }
            }
        }

        let post_type = if fm.post_type.as_deref() == Some("page") {
            PostType::Page
        } else {
            PostType::Post
        };
        let post = match posts::create_post(
            db,
            posts::NewPost {
                title,
                content_md: body.clone(),
                excerpt: None,
                slug: Some(slug),
                status: PostStatus::Published,
                post_type,
                category_id,
                column_id: None,
                tags: fm.tags.clone(),
            },
        )
        .await
        {
            Ok(p) => p,
            Err(e) => {
                report
                    .failures
                    .push(format!("{file_name}: 创建失败 {}", e.message()));
                continue;
            }
        };
        report.posts_created += 1;

        // published_at 取 front-matter date（Halo 原文发布时间），解析失败保持建文时间；
        // UPDATE 失败记录 failures，文章本身仍保留。
        if let Some(date_str) = fm.date.as_deref().and_then(parse_datetime) {
            if let Err(e) = sqlx::query("UPDATE posts SET published_at = ? WHERE id = ?")
                .bind(date_str.to_rfc3339_opts(SecondsFormat::Nanos, true))
                .bind(post.id)
                .execute(db)
                .await
            {
                report
                    .failures
                    .push(format!("{file_name}: 设置发布时间失败 {e}"));
            }
        }

        // 正文图片管线：替换 URL 后更新文章（失败不中断，post 已创建）
        let new_content =
            match process_images(&img_ctx, &body, &mut archive, &mut report, &file_name, &mut total_bytes)
                .await
            {
                Ok(c) => c,
                Err(e) => {
                    report
                        .failures
                        .push(format!("{file_name}: 图片处理失败 {}", e.message()));
                    continue;
                }
            };
        if new_content != body {
            if let Err(e) = posts::update_post(
                db,
                post.id,
                posts::UpdatePost {
                    title: None,
                    content_md: Some(new_content),
                    excerpt: None,
                    slug: None,
                    status: None,
                    post_type: None,
                    category_id: None,
                    column_id: None,
                    tags: None,
                },
            )
            .await
            {
                report
                    .failures
                    .push(format!("{file_name}: 正文更新失败 {}", e.message()));
            }
        }
    }
    Ok(report)
}

/// 图片管线的共享上下文（收束参数，规避 clippy::too_many_arguments）。
struct ImageCtx<'a> {
    db: &'a Db,
    cfg: &'a Config,
    uploads_dir: &'a Path,
    image_paths: &'a [(String, usize)],
    download_images: bool,
    /// 外部图片下载共用的 reqwest Client（30s 超时，进程内复用连接池）。
    client: &'a reqwest::Client,
}

/// 图片管线：扫描 `![alt](url)`，外部 http(s) 按需下载、zip 内相对路径解包，
/// 统一经 `save_bytes` 落库并替换 URL；失败记入报告并保留原 URL。
/// `total` 累计解压字节（I8 总量限流，与 md 条目共用同一计数器）。
async fn process_images(
    ctx: &ImageCtx<'_>,
    body: &str,
    archive: &mut zip::ZipArchive<std::fs::File>,
    report: &mut ImportReport,
    file_name: &str,
    total: &mut u64,
) -> Result<String, AppError> {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some((start, alt, url, end)) = find_image_ref(rest) {
        out.push_str(&rest[..start]);
        let replacement =
            match process_one_image(ctx, &alt, &url, archive, report, file_name, total).await {
                Ok(s) => s,
                Err(e) => {
                    report.images_failed += 1;
                    report.failures.push(format!(
                        "{file_name}: 图片 {url} 处理失败 {}",
                        e.message()
                    ));
                    format!("![{alt}]({url})")
                }
            };
        out.push_str(&replacement);
        rest = &rest[end..];
    }
    out.push_str(rest);
    Ok(out)
}

/// 处理单个图片引用：成功返回替换后的 `![alt](新URL)`，未导入时原样返回。
async fn process_one_image(
    ctx: &ImageCtx<'_>,
    alt: &str,
    url: &str,
    archive: &mut zip::ZipArchive<std::fs::File>,
    report: &mut ImportReport,
    file_name: &str,
    total: &mut u64,
) -> Result<String, AppError> {
    let url = url.trim();
    let original = format!("![{alt}]({url})");
    if url.starts_with("http://") || url.starts_with("https://") {
        if !ctx.download_images {
            return Ok(original); // 未开启下载：外部 URL 原样保留
        }
        return match download_image(ctx, url).await {
            Ok(path) => {
                report.images_downloaded += 1;
                Ok(format!("![{alt}](/uploads/{path})"))
            }
            Err(e) => {
                report.images_failed += 1;
                report
                    .failures
                    .push(format!("{file_name}: 下载图片 {url} 失败 {e}"));
                Ok(original)
            }
        };
    }
    // zip 内相对路径：解析条目 → 读 bytes → save_bytes
    let Some(entry_idx) = resolve_image(ctx.image_paths, url) else {
        report.images_failed += 1;
        report
            .failures
            .push(format!("{file_name}: zip 内未找到图片 {url}"));
        return Ok(original);
    };
    let mut entry = archive.by_index(entry_idx).map_err(internal)?;
    let entry_name = entry.name().to_string();
    // 单条目/总量限流（I8）：恶意 zip 可声明超大条目，读入内存前截断；
    // 总量超限后不再继续解包（先检后读，避免已超限仍读入）
    if *total > MAX_TOTAL_BYTES {
        return Err(AppError::BadRequest(
            "迁移包解压总量超过 2GB 上限，已中止".into(),
        ));
    }
    let mut bytes = Vec::new();
    let n = entry
        .by_ref()
        .take(MAX_ENTRY_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(internal)?;
    if n as u64 > MAX_ENTRY_BYTES {
        report.images_failed += 1;
        report
            .failures
            .push(format!("{file_name}: 图片 {url} 超过 500MB，跳过"));
        return Ok(original);
    }
    *total += n as u64;
    if *total > MAX_TOTAL_BYTES {
        return Err(AppError::BadRequest(
            "迁移包解压总量超过 2GB 上限，已中止".into(),
        ));
    }
    let orig_name = Path::new(&entry_name)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(&entry_name)
        .to_string();
    let Some(mime) = mime_for_name(&orig_name) else {
        report.images_failed += 1;
        report
            .failures
            .push(format!("{file_name}: 图片 {url} 格式不受支持"));
        return Ok(original);
    };
    match uploads::save_bytes(ctx.db, ctx.cfg, ctx.uploads_dir, &orig_name, mime, &bytes).await {
        Ok(att) => Ok(format!("![{alt}](/uploads/{})", att.path)),
        Err(e) => {
            report.images_failed += 1;
            report.failures.push(format!(
                "{file_name}: 图片 {url} 保存失败 {}",
                e.message()
            ));
            Ok(original)
        }
    }
}

/// 下载外部图片：reqwest GET（30s 超时，复用 ImageCtx.client）→ 按响应
/// Content-Type / URL 扩展名定 mime → save_bytes 落库，返回附件相对路径。
/// 文件名先按 `?`/`#` 截断（query/fragment 不属于文件名，否则扩展名解析失败）。
async fn download_image(ctx: &ImageCtx<'_>, url: &str) -> Result<String, String> {
    let resp = ctx
        .client
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let orig_name = clean_url_name(url);
    let mime = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .map(|s| s.trim().to_lowercase())
        .filter(|m| is_supported_mime(m))
        .or_else(|| mime_for_name(&orig_name).map(str::to_string))
        .ok_or_else(|| "无法识别图片格式".to_string())?;
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    let att = uploads::save_bytes(ctx.db, ctx.cfg, ctx.uploads_dir, &orig_name, &mime, &bytes)
        .await
        .map_err(|e| e.message().to_string())?;
    Ok(att.path)
}

/// 从 URL 取文件名：截断 `?`/`#` 之后的 query/fragment（`.../x.png?v=2` → `x.png`）。
fn clean_url_name(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|n| !n.is_empty())
        .unwrap_or("image")
        .to_string()
}

/// zip 内相对路径解析：精确路径 → 后缀匹配（`local/1.png` 命中 `assets/local/1.png`）
/// → 文件名匹配；仅当唯一命中才返回，避免歧义。
fn resolve_image(image_paths: &[(String, usize)], ref_path: &str) -> Option<usize> {
    let norm = normalize_path(ref_path);
    if let Some((_, i)) = image_paths.iter().find(|(p, _)| p == &norm) {
        return Some(*i);
    }
    let suffix = format!("/{norm}");
    let matches: Vec<usize> = image_paths
        .iter()
        .filter(|(p, _)| p.ends_with(&suffix))
        .map(|(_, i)| *i)
        .collect();
    if matches.len() == 1 {
        return Some(matches[0]);
    }
    let base = norm.rsplit('/').next().unwrap_or(&norm);
    let matches: Vec<usize> = image_paths
        .iter()
        .filter(|(p, _)| p.rsplit('/').next() == Some(base))
        .map(|(_, i)| *i)
        .collect();
    if matches.len() == 1 {
        return Some(matches[0]);
    }
    None
}

/// 扫描正文中下一个 `![alt](url)`，返回 (起始偏移, alt, url, 结束偏移)。
fn find_image_ref(s: &str) -> Option<(usize, String, String, usize)> {
    for (i, _) in s.char_indices() {
        if !s[i..].starts_with("![") {
            continue;
        }
        let alt_start = i + 2;
        let Some(rel_close) = s[alt_start..].find(']') else {
            continue;
        };
        let alt_end = alt_start + rel_close;
        if !s[alt_end..].starts_with("](") {
            continue;
        }
        let url_start = alt_end + 2;
        let Some(rel_paren) = s[url_start..].find(')') else {
            continue;
        };
        let end = url_start + rel_paren + 1;
        return Some((
            i,
            s[alt_start..alt_end].to_string(),
            s[url_start..url_start + rel_paren].to_string(),
            end,
        ));
    }
    None
}

/// 归一化条目/引用路径：`\` → `/`、去掉前导 `./`、百分号解码（`%20` 等）。
fn normalize_path(p: &str) -> String {
    let mut s = p.replace('\\', "/");
    while let Some(rest) = s.strip_prefix("./") {
        s = rest.to_string();
    }
    percent_decode(&s)
}

/// 极简百分号解码（zip 条目名是原始字节，正文引用可能是 URL 编码）。
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = hex_val(bytes[i + 1]);
            let lo = hex_val(bytes[i + 2]);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// 按文件名扩展名判定 mime（附件白名单内）。
fn mime_for_name(name: &str) -> Option<&'static str> {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        _ => None,
    }
}

fn is_supported_mime(mime: &str) -> bool {
    matches!(mime, "image/png" | "image/jpeg" | "image/webp" | "image/gif")
}

/// 分类按名称 ensure：slug 已存在则复用，否则创建（sort_order 0，后台可再调整）。
async fn ensure_category(db: &Db, name: &str) -> Result<crate::models::Category, AppError> {
    let slug = posts::slugify(name).await;
    if let Some(c) = taxonomy::get_category_by_slug(db, &slug).await? {
        return Ok(c);
    }
    taxonomy::create_category(db, name, &slug, 0).await
}

/// 正文首行一级标题（`# ` 开头）作为无 front-matter 时的标题。
fn first_heading(body: &str) -> Option<String> {
    body.lines()
        .map(str::trim)
        .find(|l| l.starts_with("# "))
        .map(|l| l.trim_start_matches('#').trim().to_string())
}

/// 文件名（去掉目录与 .md 后缀）作为无 slug 时的 slug 与兜底标题。
fn file_stem(name: &str) -> String {
    Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(name)
        .to_string()
}

/// front-matter date → UTC 时间（Halo 导出多为本地时间字符串，按 UTC 存储，
/// 展示侧经配置时区换算）。支持 `YYYY-MM-DD HH:MM:SS` / `HH:MM` / `T` 分隔 / 仅日期。
fn parse_datetime(s: &str) -> Option<DateTime<Utc>> {
    let s = s.trim();
    const WITH_TIME: [&str; 4] = [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
    ];
    for f in WITH_TIME {
        if let Ok(dt) = NaiveDateTime::parse_from_str(s, f) {
            return Some(dt.and_utc());
        }
    }
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|dt| dt.and_utc())
}

fn internal(e: impl std::fmt::Display) -> AppError {
    AppError::Internal(e.to_string())
}
