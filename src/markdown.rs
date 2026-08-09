//! Markdown 渲染：pulldown-cmark 最小封装。
//!
//! 本任务仅需默认选项的最小渲染，T7 再完善选项（表格、任务列表、标题 id 等）。

use pulldown_cmark::{Options, Parser, html};

/// 将 Markdown 渲染为 HTML，包裹在 `<div class="md-body">…</div>` 中。
pub fn render(md: &str) -> String {
    let parser = Parser::new_ext(md, Options::empty());
    let mut body = String::with_capacity(md.len() * 2);
    html::push_html(&mut body, parser);
    format!("<div class=\"md-body\">{body}</div>")
}
