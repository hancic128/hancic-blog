//! 主题系统：主题发现、元信息加载与 tera 模板构建。

use chrono::FixedOffset;
use std::path::{Path, PathBuf};
use tera::{Error as TeraError, Kwargs, State, Tera, TeraResult, Value};

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
    tera.register_filter("date", date_filter);
    tera.load_from_glob(&format!("{}/**/*.html", tpl_dir.display()))
        .map_err(|e| e.to_string())?;
    Ok(tera)
}

/// 主题静态资源目录（URL 前缀约定 `/theme/<name>/static/`）。
pub fn static_dir(themes_dir: &Path, name: &str) -> PathBuf {
    themes_dir.join(name).join("static")
}

/// markdown 过滤器：渲染 Markdown 为 HTML，并标记为安全（不参与自动转义）。
fn markdown_filter(value: String, _kwargs: Kwargs, _state: &State) -> TeraResult<Value> {
    Ok(Value::safe_string(&crate::markdown::render(&value)))
}

/// date 过滤器：把 RFC3339 时间字符串按站点时区（当前固定 Asia/Shanghai = UTC+8）
/// 格式化为 `YYYY-MM-DD HH:MM`。tera 上下文中 chrono DateTime<Utc> 经 serde 序列化为
/// RFC3339 字符串，故入参为字符串。
fn date_filter(value: &str, _kwargs: Kwargs, _state: &State) -> TeraResult<Value> {
    let dt = chrono::DateTime::parse_from_rfc3339(value)
        .map_err(|e| TeraError::message(format!("date 过滤器无法解析 {value:?}: {e}")))?;
    // Asia/Shanghai 全年为 UTC+8（无夏令时），T17 允许配置时区后再支持任意 tz。
    let tz = FixedOffset::east_opt(8 * 3600).expect("UTC+8 偏移量合法");
    Ok(Value::from(
        dt.with_timezone(&tz).format("%Y-%m-%d %H:%M").to_string(),
    ))
}
