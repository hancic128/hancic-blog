//! 主题系统测试：discover / build_tera / 缺失主题报错。
mod common;
use common::{copy_recursive, test_config};
use hancic::themes;

#[tokio::test]
async fn discover_and_build_default_theme() {
    let cfg = test_config("themes");
    let themes_dir = cfg.data_dir.join("themes");
    std::fs::create_dir_all(&themes_dir).unwrap();
    copy_recursive("themes/default", &themes_dir.join("default"));
    let metas = themes::discover(&themes_dir).unwrap();
    assert!(metas.iter().any(|m| m.name == "default"));
    let tera = themes::build_tera(&themes_dir, "default").unwrap();
    let names: Vec<&str> = tera.get_template_names().collect();
    assert!(names.contains(&"index.html"));
    assert!(names.contains(&"partials/header.html"));
}

#[tokio::test]
async fn missing_theme_errors() {
    let cfg = test_config("themes-missing");
    let r = themes::build_tera(&cfg.data_dir.join("themes"), "nonexistent");
    assert!(r.is_err());
}
