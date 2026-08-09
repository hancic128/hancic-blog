//! Markdown 渲染：pulldown-cmark 封装。
//!
//! 开启表格、删除线、标题属性（id）、脚注与任务列表；输出统一包裹
//! `<div class="md-body">…</div>`，供主题模板 `| markdown | safe` 过滤器调用。

use pulldown_cmark::{Options, Parser, html};

/// 将 Markdown 渲染为 HTML，包裹在 `<div class="md-body">…</div>` 中。
pub fn render(md: &str) -> String {
    let mut opts = Options::empty();
    opts.insert(
        Options::ENABLE_TABLES
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_HEADING_ATTRIBUTES
            | Options::ENABLE_FOOTNOTES
            | Options::ENABLE_TASKLISTS,
    );
    let parser = Parser::new_ext(md, opts);
    let mut body = String::with_capacity(md.len() * 2);
    html::push_html(&mut body, parser);
    format!("<div class=\"md-body\">{body}</div>")
}
