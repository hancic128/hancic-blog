use hancic::config::Config;
use std::path::PathBuf;

pub fn temp_data_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hancic-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn test_config(tag: &str) -> Config {
    Config::default_for_temp_dir().with_data_dir(temp_data_dir(tag))
}

pub async fn test_app(tag: &str) -> axum::Router {
    hancic::app(test_config(tag))
}
