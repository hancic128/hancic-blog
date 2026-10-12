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
        assert_eq!(
            (toc[0].level, toc[0].text.as_str(), toc[0].id),
            (2, "第一节", 0)
        );
        assert_eq!(
            (toc[1].level, toc[1].text.as_str(), toc[1].id),
            (3, "子节一", 1)
        );
        assert_eq!(
            (toc[2].level, toc[2].text.as_str(), toc[2].id),
            (2, "第二节", 2)
        );
        assert!(
            html.contains(r#"<span id="toc-0"></span>"#),
            "h2 前应有锚点"
        );
        assert!(
            html.contains(r#"<span id="toc-1"></span>"#),
            "h3 前应有锚点"
        );
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

    #[test]
    fn bold_next_to_chinese_renders_not_as_source() {
        // CJK 汉字既非空白也非标点，在 pulldown 的 `*` 分隔符规则里被视为词内字符，
        // 因此 `**` 紧贴汉字仍构成合法强调（无需预处理）
        let html = render("中文**加粗**结尾");
        assert!(!html.contains("**"), "不应残留 ** 源码");
        assert!(html.contains("<strong>加粗</strong>"), "应渲染为 <strong>");
        // 右边界紧贴汉字
        let html = render("**加粗**后接汉字");
        assert!(!html.contains("**"), "不应残留 ** 源码");
        assert!(html.contains("<strong>加粗</strong>"));
        // 两侧都紧贴
        let html = render("首**中间**尾");
        assert!(!html.contains("**"));
        assert!(html.contains("<strong>中间</strong>"));
    }

    #[test]
    fn bold_ascii_adjacent_renders() {
        let html = render("a**b**c");
        assert!(!html.contains("**"));
        assert!(html.contains("<strong>b</strong>"));
    }

    #[test]
    fn bold_standard_writing_unchanged() {
        // 独立成词、标点边界、链接内、列表内：应正常渲染
        for md in [
            "**加粗**",
            "，**加粗**。",
            "[**加粗**](url)",
            "- **加粗**",
            "## **加粗**",
        ] {
            let html = render(md);
            assert!(html.contains("<strong>加粗</strong>"), "case: {md}");
            assert!(!html.contains("**"), "case: {md}");
        }
    }

    #[test]
    fn bold_renders_even_with_earlier_lone_asterisk() {
        // 回归：之前的「补空格」预处理用全局星号奇偶配对判定 opener/closer，
        // 一旦文档中出现先于加粗的单个 `*`（乘法、脚注星号、单星号斜体），
        // 配对错位会把空格插到 `**加粗**` 内侧，导致加粗显示为字面源码。
        for md in [
            "3 * 4 和 **加粗**",
            "注 * 见下\n**加粗**内容",
            "价格 3 * 4 元\n\n中文**加粗**结尾",
        ] {
            let html = render(md);
            assert!(html.contains("<strong>加粗</strong>"), "case: {md}");
            assert!(!html.contains("**"), "case: {md}");
        }
    }

    #[test]
    fn italic_and_bold_mixed_render_independently() {
        // 单星号斜体不再干扰后续 `**加粗**` 的解析
        let html = render("中文*斜体*与**加粗**混排");
        assert!(html.contains("<em>斜体</em>"), "斜体应渲染");
        assert!(html.contains("<strong>加粗</strong>"), "加粗应渲染");
    }

    #[test]
    fn bold_inside_code_kept_literal() {
        // 围栏代码块与行内代码内的 * 原样保留（本就由 parser 处理，无需额外逻辑）
        let md = "```rust\nlet x = a**b**c;\n```\n\n正文 **加粗** `a**b**c` 结束。";
        let html = render(md);
        assert!(html.contains("a**b**c"), "代码内 ** 应原样保留");
        assert!(
            html.contains("<code>a**b**c</code>"),
            "行内代码内 ** 应原样保留"
        );
        // 正文中正常的强调不受影响
        assert!(html.contains("<strong>加粗</strong>"));
    }

    #[test]
    fn bold_after_code_block_renders() {
        // 围栏代码块之后的正文强调正常渲染
        let md = "```\ncode **here**\n```\n后续**加粗**文";
        let html = render(md);
        assert!(html.contains("code **here**"), "代码内 ** 应原样保留");
        assert!(!html.contains("后续**"), "正文 ** 不应残留源码");
        assert!(
            html.contains("<strong>加粗</strong>"),
            "代码块后的强调应正常渲染"
        );
    }
}
