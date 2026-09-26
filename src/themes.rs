//! 主题系统：主题发现、元信息加载与 tera 模板构建。

use chrono::FixedOffset;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tera::{Error as TeraError, Kwargs, State, Tera, TeraResult, Value};
use tokio::sync::RwLock;

/// 主题元信息（来自主题目录下的 theme.toml，缺省字段为空串）。
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ThemeMeta {
    pub name: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
}

/// 扫描主题目录，读取每个子目录的 theme.toml；损坏的目录跳过并 warn。
pub fn discover(themes_dir: &Path) -> Result<Vec<ThemeMeta>, String> {
    let mut out = vec![];
    for e in std::fs::read_dir(themes_dir).map_err(|e| e.to_string())?.flatten() {
        if !e.path().is_dir() {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        match load_meta(themes_dir, &name) {
            Ok(m) => out.push(m),
            Err(err) => tracing::warn!("跳过无效主题 {name}: {err}"),
        }
    }
    Ok(out)
}

/// 读取单个主题的元信息；theme.toml 缺失或解析失败时报错。
pub fn load_meta(themes_dir: &Path, name: &str) -> Result<ThemeMeta, String> {
    let f = themes_dir.join(name).join("theme.toml");
    let content = std::fs::read_to_string(&f).map_err(|e| format!("{f:?}: {e}"))?;
    let mut meta: ThemeMeta = toml::from_str(&content).map_err(|e| e.to_string())?;
    meta.name = name.to_string(); // 以目录名为准
    Ok(meta)
}

/// 构建某主题的 tera：模板根为该主题 `templates/`（含 partials），并注册 markdown/date 过滤器。
pub fn build_tera(themes_dir: &Path, name: &str) -> Result<Tera, String> {
    let tpl_dir = themes_dir.join(name).join("templates");
    if !tpl_dir.is_dir() {
        return Err(format!(
            "主题 {name} 的模板目录不存在: {}",
            tpl_dir.display()
        ));
    }
    // tera 2.x 用 Tera::default() + load_from_glob（tera 1.x 的 Tera::new(glob) 已移除）。
    // 注意：tera 2.x 在 load_from_glob 时即校验模板引用的过滤器，必须「先注册、后加载」。
    let mut tera = Tera::default();
    tera.register_filter("markdown", markdown_filter);
    tera.register_filter("markdown_breaks", markdown_breaks_filter);
    tera.register_filter("date", date_filter);
    tera.load_from_glob(&format!("{}/**/*.html", tpl_dir.display()))
        .map_err(|e| e.to_string())?;
    Ok(tera)
}

/// 主题静态资源目录（URL 前缀约定 `/theme/<name>/static/`）。
pub fn static_dir(themes_dir: &Path, name: &str) -> PathBuf {
    themes_dir.join(name).join("static")
}

/// 主题名白名单：仅 ASCII 字母数字与 `-`/`_`，防止路径穿越。
/// 前台静态资源路由与后台主题管理共用。
pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// 主题缓存条目：`Arc<Tera>` + 模板目录 mtime（用于「文件被改」自动失效）。
struct CacheEntry {
    tera: Arc<Tera>,
    /// 模板目录及其子文件的最大 mtime；用于检测文件被改后自动重建。
    templates_mtime: Option<std::time::SystemTime>,
}

/// 主题 Tera 缓存：按主题名缓存已构建的 `Tera` 实例，避免每次请求重建。
/// 触发重建的时机：
/// - 后台切换主题（修改 `settings.active_theme`）：目标主题名未命中→重建；
/// - 重新导入主题（admin `import`）：调用 `invalidate` 显式清掉；
/// - 模板文件被改（mtime 变化）：下次请求 mtime 校验失败→重建。
///
/// 并发安全：`get_or_build` 采用读锁快路径 + 写锁双检模式；
/// 磁盘 I/O（构建 Tera、mtime 扫描）发生在锁外，避免长时间持锁。
#[derive(Default)]
pub struct ThemeTeraCache {
    inner: RwLock<HashMap<String, CacheEntry>>,
}

impl ThemeTeraCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// 删除指定主题的缓存（uninstall / 重新导入后强制重建用）。
    pub async fn invalidate(&self, name: &str) {
        self.inner.write().await.remove(name);
    }

    /// 取缓存；命中且 mtime 未变则复用，否则重建。主题名非法或模板加载失败时返回错误。
    ///
    /// 流程：① 锁外算当前 mtime；② 读锁快路径（mtime 一致即返回 Arc）；
    /// ③ 锁外构建新 Tera；④ 写锁双检并插入。
    pub async fn get_or_build(
        &self,
        themes_dir: &Path,
        name: &str,
    ) -> Result<Arc<Tera>, String> {
        let tpl_dir = themes_dir.join(name).join("templates");
        let current_mtime = templates_max_mtime(&tpl_dir);

        // ① 读锁快路径
        {
            let guard = self.inner.read().await;
            if let Some(entry) = guard.get(name) {
                if entry.templates_mtime == current_mtime {
                    return Ok(entry.tera.clone());
                }
                // mtime 变了，落到下面重建
            }
        }

        // ② 锁外构建
        let tera = build_tera(themes_dir, name)?;

        // ③ 写锁双检 + 写入
        let mut guard = self.inner.write().await;
        if let Some(entry) = guard.get(name) {
            if entry.templates_mtime == current_mtime {
                return Ok(entry.tera.clone());
            }
        }
        let arc = Arc::new(tera);
        guard.insert(
            name.to_string(),
            CacheEntry {
                tera: arc.clone(),
                templates_mtime: current_mtime,
            },
        );
        Ok(arc)
    }
}

/// 递归扫描 templates 目录，计算所有文件的最大 mtime。
/// 目录不存在 / 权限错误时返回 `None`（让调用方走「无 mtime」路径，
/// 即不命中任何缓存，每次重建——主题真的不存在时 `build_tera` 会失败并报错）。
fn templates_max_mtime(dir: &Path) -> Option<std::time::SystemTime> {
    use std::time::SystemTime;
    fn walk(d: &Path, cur: &mut SystemTime) -> std::io::Result<()> {
        for entry in std::fs::read_dir(d)? {
            let entry = entry?;
            let path = entry.path();
            let mt = if path.is_dir() {
                walk(&path, cur)?;
                entry.metadata()?.modified().ok()
            } else {
                entry.metadata()?.modified().ok()
            };
            if let Some(mt) = mt {
                if mt > *cur {
                    *cur = mt;
                }
            }
        }
        Ok(())
    }
    let mut max = SystemTime::UNIX_EPOCH;
    if walk(dir, &mut max).is_err() {
        return None;
    }
    if max == SystemTime::UNIX_EPOCH {
        return None;
    }
    Some(max)
}

/// 安装主题：解压 zip 到临时目录 → 校验 theme.toml/目录名/模板可构建 →
/// 原子替换到 `themes_dir/{name}`（同名覆盖旧版本）。返回安装后的主题元信息。
///
/// zip 内 theme.toml 可位于根目录或单层主题目录（如 `mytheme/theme.toml`），
/// 解压时剥掉该顶层前缀；含非法路径（`..`/`\`/绝对路径）的条目直接拒绝。
pub fn install(themes_dir: &Path, zip_bytes: &[u8]) -> Result<ThemeMeta, String> {
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).map_err(|e| e.to_string())?;

    // 1. 定位 theme.toml 并推断顶层前缀（String 持有，避免借用 archive 内 ZipFile）
    let mut root_prefix: Option<String> = None;
    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name();
        if !entry_name_is_safe(name) {
            return Err("主题包内含非法路径（如 .. 或 \\）".into());
        }
        if name.ends_with("theme.toml") {
            let prefix = name.trim_end_matches("theme.toml").trim_end_matches('/');
            root_prefix = Some(prefix.to_string());
        }
    }
    let Some(prefix) = root_prefix.as_deref() else {
        return Err("主题包中未找到 theme.toml".into());
    };

    // 2. 解压到临时目录（时间戳命名避免并发冲突；用完即删）
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = themes_dir.join(format!(".install-{ts}"));
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let cleanup = |tmp: &Path| {
        if tmp.exists() {
            let _ = std::fs::remove_dir_all(tmp);
        }
    };
    let result = (|| -> Result<ThemeMeta, String> {
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
            let name = entry.name().to_string();
            if !entry_name_is_safe(&name) {
                return Err("主题包内含非法路径（如 .. 或 \\）".into());
            }
            // 剥顶层前缀：`{prefix}/rest` → `tmp/rest`
            let rest = match prefix.is_empty() {
                true => name.as_str(),
                false => name.strip_prefix(&format!("{prefix}/")).unwrap_or(&name),
            };
            if rest.is_empty() {
                continue; // 顶层目录条目本身
            }
            let dest = tmp.join(rest);
            if entry.is_dir() {
                std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
                continue;
            }
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut out = std::fs::File::create(&dest).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
        }
        // 3. 确定主题名并校验
        let name = if prefix.is_empty() {
            parse_meta_name(&tmp)?.ok_or_else(|| "theme.toml 缺少 name 字段".to_string())?
        } else {
            prefix.to_string()
        };
        if !is_valid_name(&name) {
            return Err("主题名不合法（仅限字母/数字/-/_）".into());
        }
        // 4. 原子替换：删除旧版本 → 移动临时目录 → 校验模板可构建（失败回滚删除）
        let dest = themes_dir.join(&name);
        if dest.exists() {
            std::fs::remove_dir_all(&dest).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
        if let Err(e) = build_tera(themes_dir, &name) {
            let _ = std::fs::remove_dir_all(&dest);
            return Err(format!("主题模板无效: {e}"));
        }
        load_meta(themes_dir, &name)
    })();
    if result.is_err() {
        cleanup(&tmp);
    }
    result
}

/// 读取临时解压目录内 theme.toml 的 `name` 字段（解压后目录名非主题名时用）。
fn parse_meta_name(dir: &Path) -> Result<Option<String>, String> {
    let content =
        std::fs::read_to_string(dir.join("theme.toml")).map_err(|e| e.to_string())?;
    #[derive(serde::Deserialize)]
    struct MetaName {
        name: Option<String>,
    }
    let meta: MetaName = toml::from_str(&content).map_err(|e| e.to_string())?;
    Ok(meta.name)
}

/// zip 条目名安全校验：拒绝反斜杠/NUL，且所有路径组件必须是普通组件
/// （绝对路径、`..`、当前目录均拒绝，防 zip-slip 穿越）。
fn entry_name_is_safe(name: &str) -> bool {
    if name.contains('\\') || name.contains('\0') {
        return false;
    }
    Path::new(name)
        .components()
        .all(|c| matches!(c, std::path::Component::Normal(_)))
}

/// markdown 过滤器：渲染 Markdown 为 HTML，并标记为安全（不参与自动转义）。
fn markdown_filter(value: String, _kwargs: Kwargs, _state: &State) -> TeraResult<Value> {
    Ok(Value::safe_string(&crate::markdown::render(&value)))
}

/// markdown_breaks 过滤器：同 markdown，但软换行（单 `\n`）也渲染为 `<br>`，
/// 供说说等短文本保留换行与段落结构。
fn markdown_breaks_filter(value: String, _kwargs: Kwargs, _state: &State) -> TeraResult<Value> {
    Ok(Value::safe_string(&crate::markdown::render_breaks(&value)))
}

/// date 过滤器：把 RFC3339 时间字符串按站点时区（当前固定 Asia/Shanghai = UTC+8）
/// 格式化为 `YYYY-MM-DD HH:MM`（默认）。支持 `fmt` 参数映射 settings 的
/// `date_format`：`date` = 仅日期（`YYYY-MM-DD`），其余值回退含时间。
/// tera 上下文中 chrono DateTime<Utc> 经 serde 序列化为 RFC3339 字符串，故入参为字符串。
fn date_filter(value: &str, kwargs: Kwargs, _state: &State) -> TeraResult<Value> {
    let dt = chrono::DateTime::parse_from_rfc3339(value)
        .map_err(|e| TeraError::message(format!("date 过滤器无法解析 {value:?}: {e}")))?;
    // Asia/Shanghai 全年为 UTC+8（无夏令时），T17 允许配置时区后再支持任意 tz。
    let tz = FixedOffset::east_opt(8 * 3600).expect("UTC+8 偏移量合法");
    let fmt = match kwargs
        .get("fmt")
        .ok()
        .flatten()
        .and_then(Value::as_str)
        .unwrap_or("datetime")
    {
        "date" => "%Y-%m-%d",
        _ => "%Y-%m-%d %H:%M",
    };
    Ok(Value::from(
        dt.with_timezone(&tz).format(fmt).to_string(),
    ))
}
