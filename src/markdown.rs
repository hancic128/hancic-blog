//! Markdown 渲染：pulldown-cmark 封装。
//!
//! 开启表格、删除线、标题属性（id）、脚注与任务列表；输出统一包裹
//! `<div class="md-body">…</div>`，供主题模板 `| markdown | safe` 过滤器调用。
//! `render_with_toc` 额外为 1~3 级标题插入隐藏锚点（`#toc-N`），返回目录项，
//! 供文章页右侧目录导航使用。

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd, html};

/// 目录项：标题级别（1=H1..3=H3）、文本、锚点 id。
#[derive(Debug, Clone)]
pub struct TocItem {
    pub level: u8,
    pub text: String,
    pub id: usize,
}

/// 将 Markdown 渲染为 HTML，包裹在 `<div class="md-body">…</div>` 中。
pub fn render(md: &str) -> String {
    render_with_toc(md).0
}

/// 渲染 Markdown 为 HTML（软换行也渲染为 `<br>`），用于说说等短文本。
/// 普通 Markdown 段落（空行分隔）照常渲染；单换行不再被折叠成空格。
/// 通过事件变换把 `SoftBreak` 替换为 `HardBreak`，代码块内的换行不受影响
/// （0.13 无 `ENABLE_SOFT_BREAKS` 选项）。
pub fn render_breaks(md: &str) -> String {
    let opts = markdown_options();
    let parser = Parser::new_ext(md, opts);
    let events = parser.map(|ev| match ev {
        Event::SoftBreak => Event::HardBreak,
        other => other,
    });
    let mut body = String::with_capacity(md.len() * 2);
    html::push_html(&mut body, events);
    format!("<div class=\"md-body\">{body}</div>")
}

/// 渲染 Markdown 并提取目录：为 1~3 级标题插入 `<span id="toc-N">` 锚点
/// （标题文本经 HTML 转义，防止目录注入）。
pub fn render_with_toc(md: &str) -> (String, Vec<TocItem>) {
    let opts = markdown_options();

    // 第一遍：按出现顺序收集 1~3 级标题
    let mut toc: Vec<TocItem> = Vec::new();
    {
        let parser = Parser::new_ext(md, opts);
        let mut cur: Option<(u8, String)> = None;
        for ev in parser {
            match ev {
                Event::Start(Tag::Heading { level, .. }) => {
                    cur = Some((heading_level(level), String::new()));
                }
                Event::Text(t) => {
                    if let Some((_, text)) = cur.as_mut() {
                        text.push_str(&t);
                    }
                }
                Event::End(TagEnd::Heading(_)) => {
                    if let Some((level, text)) = cur.take() {
                        if (1..=3).contains(&level) {
                            let text = text.trim().to_string();
                            if !text.is_empty() {
                                toc.push(TocItem {
                                    level,
                                    text,
                                    id: toc.len(),
                                });
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // 第二遍：渲染，并在每个 1~3 级标题前插入锚点 span（序号与 toc 一一对应）
    let parser = Parser::new_ext(md, opts);
    let mut heading_seq = 0usize;
    let events = parser.flat_map(|ev| match ev {
        Event::Start(Tag::Heading { level, .. }) if (1..=3).contains(&heading_level(level)) => {
            let idx = heading_seq;
            heading_seq += 1;
            let span = Event::InlineHtml(format!("<span id=\"toc-{idx}\"></span>").into());
            vec![span, ev].into_iter()
        }
        _ => vec![ev].into_iter(),
    });
    let mut body = String::with_capacity(md.len() * 2);
    html::push_html(&mut body, events);
    (format!("<div class=\"md-body\">{body}</div>"), toc)
}

fn markdown_options() -> Options {
    let mut opts = Options::empty();
    opts.insert(
        Options::ENABLE_TABLES
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_HEADING_ATTRIBUTES
            | Options::ENABLE_FOOTNOTES
            | Options::ENABLE_TASKLISTS,
    );
    opts
}

fn heading_level(l: HeadingLevel) -> u8 {
    match l {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toc_extracts_h2_h3_with_ids() {
        let md = "## 第一节\n\n正文。\n\n### 子节一\n\n内容。\n\n## 第二节\n\n内容。\n\n#### 不收录\n\n内容。";
        let (html, toc) = render_with_toc(md);
        assert_eq!(toc.len(), 3, "应提取 3 个 1~3 级标题");
        assert_eq!((toc[0].level, toc[0].text.as_str(), toc[0].id), (2, "第一节", 0));
        assert_eq!((toc[1].level, toc[1].text.as_str(), toc[1].id), (3, "子节一", 1));
        assert_eq!((toc[2].level, toc[2].text.as_str(), toc[2].id), (2, "第二节", 2));
        assert!(html.contains(r#"<span id="toc-0"></span>"#), "h2 前应有锚点");
        assert!(html.contains(r#"<span id="toc-1"></span>"#), "h3 前应有锚点");
        assert!(!html.contains(r#"toc-3"#), "h4 不应有锚点");
    }

    #[test]
    fn toc_preserves_title_text_for_template_escaping() {
        // 标题中的 HTML 标签被 pulldown 作为 HTML 事件剔除，目录文本不含可注入内容；
        // 其余文本由模板 `{{ item.text }}` 输出时 tera 自动转义
        let md = "## 标题 <script>alert(1)</script>";
        let (_, toc) = render_with_toc(md);
        assert_eq!(toc[0].text, "标题 alert(1)");
        // 渲染出的锚点仍正确插入
        let (html, _) = render_with_toc(md);
        assert!(html.contains(r#"<span id="toc-0"></span>"#));
    }

    #[test]
    fn render_keeps_table_and_code() {
        let md = "| a | b |\n|---|---|\n| 1 | 2 |\n\n```rust\nfn main() {}\n```";
        let html = render(md);
        assert!(html.contains("<table>"), "应渲染表格");
        assert!(html.contains("<pre>"), "应渲染代码块");
    }

    #[test]
    fn render_breaks_turns_soft_newlines_into_br() {
        let html = render_breaks("第一行\n第二行\n\n新段落");
        assert!(html.contains("第一行<br"), "软换行应渲染为 <br>");
        assert!(html.contains("第二行"), "第二行内容应保留");
        assert!(html.contains("<p>新段落</p>"), "空行分隔的段落应保留");
    }
}
