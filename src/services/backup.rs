//! 全量备份与恢复服务：zip 导出（config.toml + DB 快照 + uploads/ + themes/ + meta.json）
//! 与 zip 导入（校验 → 现有数据目录改名保留 → 解包替换）。
//!
//! DB 快照用 SQLite `VACUUM INTO`：同一一致性快照，无需手动备份 API；`VACUUM`
//! 不接受绑定参数，故用 `sqlx::raw_sql` + 单引号转义拼接目标路径。
//! zip 用 `zip` 8 的 `ZipWriter` / `ZipArchive`；解包路径穿越防护：拒绝含 `\`
//! 的条目名 + `enclosed_name()` + 全部普通组件校验（见 `entry_name_is_safe`）。

use crate::error::AppError;
use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use super::{MAX_ENTRY_BYTES, MAX_TOTAL_BYTES};

/// 备份 zip 内固定条目名。
const DB_ENTRY: &str = "hancic.db";
const CONFIG_ENTRY: &str = "config.toml";
const META_ENTRY: &str = "meta.json";
/// 备份的目录（zip 内条目带该前缀）。
const DIR_ENTRIES: [&str; 2] = ["uploads", "themes"];

/// 备份报告：zip 路径、字节大小与各部分文件数。
#[derive(Debug, Clone, Serialize)]
pub struct BackupReport {
    pub path: PathBuf,
    pub size: u64,
    pub counts: BackupCounts,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct BackupCounts {
    /// zip 内文件条目总数（含 db / config.toml / meta.json）。
    pub files: usize,
    /// uploads/ 下文件数。
    pub uploads: usize,
    /// themes/ 下文件数。
    pub themes: usize,
}

/// 恢复报告：各部分恢复情况与原数据目录改名位置。
#[derive(Debug, Clone, Default, Serialize)]
pub struct RestoreReport {
    /// 实际解包写入的文件数。
    pub files: usize,
    pub uploads: usize,
    pub themes: usize,
    pub config: bool,
    pub db: bool,
    /// 原数据目录改名后的位置（`<data_dir>.bak-<ts>`）。
    pub backup_dir: PathBuf,
}

/// 全量导出：`VACUUM INTO` 生成 DB 一致性快照到临时目录 → 打 zip 到 `out_zip`。
///
/// zip 结构：`config.toml`（存在时）、`hancic.db`（快照）、`uploads/`、`themes/`、
/// `meta.json`（`{version, exported_at}`）。
pub async fn export_all(data_dir: &Path, out_zip: &Path) -> Result<BackupReport, AppError> {
    // 1. VACUUM INTO 一致性快照（读到 WAL 内最新已提交数据）
    let db_path = data_dir.join(DB_ENTRY);
    if !db_path.is_file() {
        return Err(AppError::BadRequest("数据库文件不存在，无法备份".into()));
    }
    let snap_dir = std::env::temp_dir().join(format!("hancic-backup-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&snap_dir).map_err(internal)?;
    let snap_db = snap_dir.join(DB_ENTRY);
    // 池执行：sqlx 0.8 的 `&mut SqliteConnection` + `Executor` 在 boxed future 场景
    // 非 Send（handler 需要 Send future），池模式与 `db::init` 一致且 Send。
    let pool = open_pool(&db_path).await?;
    let into_path = snap_db.display().to_string().replace('\'', "''");
    sqlx::raw_sql(&format!("VACUUM INTO '{into_path}'"))
        .execute(&pool)
        .await
        .map_err(internal)?;
    pool.close().await;

    // 2. 打 zip（快照目录结束后清理，失败路径也清理）
    let mut counts = BackupCounts::default();
    let result = (|| -> Result<BackupReport, AppError> {
        let file = std::fs::File::create(out_zip).map_err(internal)?;
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();

        if let Ok(bytes) = std::fs::read(data_dir.join(CONFIG_ENTRY)) {
            writer.start_file(CONFIG_ENTRY, options).map_err(internal)?;
            writer.write_all(&bytes).map_err(internal)?;
            counts.files += 1;
        }
        writer.start_file(DB_ENTRY, options).map_err(internal)?;
        let snap_bytes = std::fs::read(&snap_db).map_err(internal)?;
        writer.write_all(&snap_bytes).map_err(internal)?;
        counts.files += 1;
        for dir in DIR_ENTRIES {
            let src = data_dir.join(dir);
            if src.is_dir() {
                let n = add_dir(&mut writer, &src, &src, dir)?;
                counts.files += n;
                if dir == "uploads" {
                    counts.uploads += n;
                } else {
                    counts.themes += n;
                }
            }
        }
        let meta = serde_json::json!({
            "version": 1,
            "exported_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        });
        writer.start_file(META_ENTRY, options).map_err(internal)?;
        writer.write_all(meta.to_string().as_bytes()).map_err(internal)?;
        counts.files += 1;
        writer.finish().map_err(internal)?;
        let size = std::fs::metadata(out_zip).map_err(internal)?.len();
        Ok(BackupReport {
            path: out_zip.to_path_buf(),
            size,
            counts,
        })
    })();
    let _ = std::fs::remove_dir_all(&snap_dir);
    result
}

/// 导出到唯一临时文件并返回 `(路径, 报告)`；调用方读取后自行删除。
pub async fn export_temp(data_dir: &Path) -> Result<(PathBuf, BackupReport), AppError> {
    let zip_path = std::env::temp_dir().join(format!("hancic-backup-{}.zip", uuid::Uuid::new_v4()));
    let report = export_all(data_dir, &zip_path).await?;
    Ok((zip_path, report))
}

/// 全量恢复：校验 zip 含 `hancic.db` + `meta.json` → 预检全部条目路径安全 →
/// WAL checkpoint → 原数据目录改名 `<data_dir>.bak-<ts>` → 解包 `uploads/` /
/// `themes/` / `config.toml` 并替换 db → 对新库跑 `PRAGMA integrity_check`（失败
/// 返回错误并提示用 .bak 回滚，I5）。
///
/// 路径安全预检先于改名执行：恶意条目在动现有数据前即被拒绝（修复审查发现的
/// zip-slip 反斜杠绕过，见 `entry_name_is_safe`）。解包按实际字节计数限流
/// （单条目 500MB / 总量 2GB，I8），超限中止并提示回滚。
///
/// zip 必须先打开再改名：备份包可能位于 data_dir 内（测试即如此），改名后
/// 原路径失效，而 POSIX 下已打开的 fd 不受改名影响，可继续读取。
pub async fn restore(data_dir: &Path, zip_path: &Path) -> Result<RestoreReport, AppError> {
    let file = std::fs::File::open(zip_path).map_err(internal)?;
    let mut archive = zip::ZipArchive::new(file).map_err(internal)?;
    let names: Vec<String> = archive.file_names().map(String::from).collect();
    if !names.iter().any(|n| n == DB_ENTRY) || !names.iter().any(|n| n == META_ENTRY) {
        return Err(AppError::BadRequest(
            "备份包格式无效：缺少 hancic.db 或 meta.json".into(),
        ));
    }
    // 预检全部条目名（在 checkpoint / 改名之前）：任一不安全条目即整体拒绝，
    // 失败时不改动现有数据
    for name in &names {
        if !entry_name_is_safe(name) {
            return Err(AppError::BadRequest(format!("备份包含非法路径: {name}")));
        }
    }

    // WAL 下未收拢的数据在 -wal 文件中：先 checkpoint(TRUNCATE) 折入主库，
    // 使改名后的 .bak 目录自洽；连接池闲置时可直接收拢，忙则跳过（整体仍一致）。
    let db_path = data_dir.join(DB_ENTRY);
    if db_path.is_file() {
        let pool = open_pool(&db_path).await?;
        let _ = sqlx::raw_sql("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&pool)
            .await;
        pool.close().await;
    }

    // 现有数据目录整体改名保留（含 -wal/-shm），再在空目录上解包
    let ts = Utc::now().format("%Y%m%d%H%M%S");
    let backup_dir = PathBuf::from(format!("{}.bak-{ts}", data_dir.display()));
    if backup_dir.exists() {
        return Err(AppError::Conflict("同名备份目录已存在，请稍后重试".into()));
    }
    std::fs::rename(data_dir, &backup_dir).map_err(internal)?;
    std::fs::create_dir_all(data_dir).map_err(internal)?;

    let mut report = RestoreReport {
        backup_dir: backup_dir.clone(),
        ..Default::default()
    };
    // 解压炸弹防护（I8）：单条目/总量上限，超限即中止——此时已改名，调用方
    // 需用 backup_dir 回滚（错误消息里带出该路径）。
    let mut total_bytes: u64 = 0;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(internal)?;
        if entry.is_dir() {
            continue;
        }
        // enclosed_name：拒绝绝对路径、NUL 与 `/` 形式的 `..` 逃逸
        let Some(rel) = entry.enclosed_name() else {
            return Err(AppError::BadRequest(format!(
                "备份包含非法路径: {}",
                entry.name()
            )));
        };
        let name = rel.to_string_lossy().replace('\\', "/");
        // 防线二：归一化后的条目路径仍须全部为普通组件（防预检被绕过的纵深防御）
        if !entry_name_is_safe(&name) {
            return Err(AppError::BadRequest(format!(
                "备份包含非法路径: {name}"
            )));
        }
        let target = match name.as_str() {
            META_ENTRY => continue, // 元数据不落盘
            DB_ENTRY => data_dir.join(DB_ENTRY),
            CONFIG_ENTRY => data_dir.join(CONFIG_ENTRY),
            _ => {
                if let Some(rest) = name.strip_prefix("uploads/") {
                    if rest.is_empty() {
                        continue;
                    }
                    data_dir.join("uploads").join(rest)
                } else if let Some(rest) = name.strip_prefix("themes/") {
                    if rest.is_empty() {
                        continue;
                    }
                    data_dir.join("themes").join(rest)
                } else {
                    tracing::warn!("跳过备份包中的未知条目: {name}");
                    continue;
                }
            }
        };
        // 双保险：enclosed_name 已保证相对路径，这里再确认未越出 data_dir
        if !target.starts_with(data_dir) {
            return Err(AppError::BadRequest(format!("备份包路径越界: {name}")));
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(internal)?;
        }
        let mut out = std::fs::File::create(&target).map_err(internal)?;
        // 按实际解压字节计数（Take 限流），防止 zip 内声明尺寸与真实不一致
        let copied = std::io::copy(&mut entry.by_ref().take(MAX_ENTRY_BYTES + 1), &mut out)
            .map_err(internal)?;
        if copied > MAX_ENTRY_BYTES {
            return Err(AppError::BadRequest(format!(
                "备份条目 {name} 超过单条目上限（{MAX_ENTRY_BYTES} 字节），已中止；\
                 原数据保留在 {}，可手动回滚",
                backup_dir.display()
            )));
        }
        total_bytes += copied;
        if total_bytes > MAX_TOTAL_BYTES {
            return Err(AppError::BadRequest(format!(
                "备份解压总量超过上限（{MAX_TOTAL_BYTES} 字节），已中止；\
                 原数据保留在 {}，可手动回滚",
                backup_dir.display()
            )));
        }
        report.files += 1;
        if name.starts_with("uploads/") {
            report.uploads += 1;
        } else if name.starts_with("themes/") {
            report.themes += 1;
        } else if name == DB_ENTRY {
            report.db = true;
        } else if name == CONFIG_ENTRY {
            report.config = true;
        }
    }
    // 恢复后校验替换的新库（I5）：integrity_check 失败即返回错误，提示用 .bak 回滚
    verify_restored_db(data_dir).await.map_err(|e| {
        AppError::Internal(format!(
            "{}（原数据保留在 {}，可手动回滚）",
            e.message(),
            backup_dir.display()
        ))
    })?;
    Ok(report)
}

/// 对替换后的新库跑 `PRAGMA integrity_check`：结果非 "ok" 视为校验失败。
async fn verify_restored_db(data_dir: &Path) -> Result<(), AppError> {
    let db_path = data_dir.join(DB_ENTRY);
    if !db_path.is_file() {
        return Err(AppError::BadRequest("备份包缺少 hancic.db".into()));
    }
    let pool = open_pool(&db_path).await?;
    let result: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&pool)
        .await
        .map_err(internal)?;
    pool.close().await;
    if result.trim() != "ok" {
        return Err(AppError::Internal(format!(
            "恢复后的数据库完整性校验失败: {result}"
        )));
    }
    Ok(())
}

/// 条目名安全校验（zip-slip 反斜杠绕过修复）：
///
/// 不能直接信任 `enclosed_name()`——它作用于原始条目名，而 macOS/Linux 上 `\`
/// 是普通文件名字符：`uploads\..\..\evil` 词法上无 `..` 组件可通过检查，待调用方
/// `replace('\\', "/")` 归一化后却构成 `uploads/../../evil` 逃逸 data_dir（甚至可
/// 覆盖 `hancic.db`）。故这里：
/// - 拒绝任何含 `\` 的条目名（本工具导出的条目名只用 `/`，合法备份不会含 `\`）
/// - 拒绝 NUL、绝对路径与一切非普通组件（`.`/`..`/根/盘符前缀）
fn entry_name_is_safe(name: &str) -> bool {
    if name.contains('\\') || name.contains('\0') {
        return false;
    }
    Path::new(name)
        .components()
        .all(|c| matches!(c, std::path::Component::Normal(_)))
}

/// 递归把 `dir` 下文件写入 zip，条目名带 `prefix` 前缀（`uploads` / `themes`）。
fn add_dir(
    writer: &mut zip::ZipWriter<std::fs::File>,
    root: &Path,
    dir: &Path,
    prefix: &str,
) -> Result<usize, AppError> {
    let mut n = 0;
    for entry in std::fs::read_dir(dir).map_err(internal)? {
        let entry = entry.map_err(internal)?;
        let path = entry.path();
        if path.is_dir() {
            n += add_dir(writer, root, &path, prefix)?;
            continue;
        }
        let rel = path.strip_prefix(root).map_err(internal)?;
        let name = format!("{prefix}/{}", rel.to_string_lossy().replace('\\', "/"));
        writer
            .start_file(name, zip::write::SimpleFileOptions::default())
            .map_err(internal)?;
        let bytes = std::fs::read(&path).map_err(internal)?;
        writer.write_all(&bytes).map_err(internal)?;
        n += 1;
    }
    Ok(n)
}

/// 打开 data_dir 下 DB 的短生命周期连接池（WAL + busy_timeout，与 `db::init` 一致）。
/// 用完即 `close()`，避免残留连接在改名数据目录时仍持有文件。
async fn open_pool(db_path: &Path) -> Result<SqlitePool, AppError> {
    let opts = SqliteConnectOptions::from_str(&format!("sqlite://{}", db_path.display()))?
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    SqlitePoolOptions::new()
        .max_connections(2)
        .connect_with(opts)
        .await
        .map_err(internal)
}

fn internal(e: impl std::fmt::Display) -> AppError {
    AppError::Internal(e.to_string())
}

