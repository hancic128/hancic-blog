use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub data_dir: PathBuf,
    /// 站点部署子路径（如 `/blog`）；空表示根路径部署。
    /// 非空时页面链接/静态资源/重定向均带此前缀，nginx 反代需剥前缀转发。
    pub base_path: String,
    pub site_url: String,
    pub site_name: String,
    pub site_desc: String,
    pub active_theme: String,
    pub image_compress: bool,
    pub image_max_edge: u32,
    pub image_quality: u8,
    pub upload_max_image: u64,
    pub upload_max_video: u64,
    pub upload_max_file: u64,
}

impl Config {
    pub fn defaults() -> Self {
        Self {
            host: "0.0.0.0".into(),
            port: 8090,
            data_dir: PathBuf::from("data"),
            base_path: String::new(),
            site_url: String::new(),
            site_name: "我的博客".into(),
            site_desc: String::new(),
            active_theme: "default".into(),
            image_compress: true,
            image_max_edge: 2000,
            image_quality: 85,
            upload_max_image: 10 * 1024 * 1024,
            upload_max_video: 100 * 1024 * 1024,
            upload_max_file: 50 * 1024 * 1024,
        }
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        // 文件不存在时全部用默认值；存在时用 toml 覆盖（serde(default) 处理缺省字段）
        match std::fs::read_to_string(path) {
            Ok(content) => toml::from_str::<Config>(&content)
                .map_err(|e| format!("解析配置文件 {} 失败: {e}", path.display())),
            Err(_) => Ok(Self::defaults()),
        }
    }

    pub fn default_for_temp_dir() -> Self {
        Self::defaults().with_data_dir(std::env::temp_dir().join("hancic-dev"))
    }

    pub fn with_data_dir(mut self, dir: PathBuf) -> Self {
        self.data_dir = dir;
        self
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::defaults()
    }
}
