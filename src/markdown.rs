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
    let md = fix_bold_boundaries(md);
    let parser = Parser::new_ext(&md, opts);
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
    let md = fix_bold_boundaries(md);

    // 第一遍：按出现顺序收集 1~3 级标题
    let mut toc: Vec<TocItem> = Vec::new();
    {
        let parser = Parser::new_ext(&md, opts);
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
    let parser = Parser::new_ext(&md, opts);
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

/// 修复强调标记的边界问题：CommonMark 规定 `**` 前后若直接贴着字母/数字/汉字
/// （非空白、非标点），该 `**` 不构成合法的强调分隔符，会原样输出为源码
/// （如 `中文**加粗**结尾` 显示为字面 `**加粗**`）。中文写作习惯常不加空格，
/// 这里在渲染前给这类 `**` 两侧补空格，让强调正常渲染。
///
/// 安全边界：围栏代码块与行内代码整段跳过，不改动其中的 `*`。
/// 非代码区所有星号 run（含单星号斜体）按出现顺序编号配对：
/// 奇数 run 视为 opener、偶数 run 视为 closer；opener 前面紧贴文字且后面
/// 非空白 → 前面补空格；closer 后面紧贴文字且前面非空白 → 后面补空格。
/// 已有的合法写法（`**加粗**`、`，**加粗**`、`[**x**](url)`、`- **x**` 等）
/// 两侧本就合法，不受影响。
fn fix_bold_boundaries(md: &str) -> String {
    let mut out = String::with_capacity(md.len() + 32);
    let mut in_fence = false;
    let mut fence_char = '\0';
    let mut star_seq = 0usize;

    for line in md.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        let trimmed = body.trim_start();
        // 围栏开启/关闭：行首（允许缩进）至少 3 个 ` 或 ~
        let (is_fence, fc) = if trimmed.len() >= 3
            && (trimmed.starts_with('`') || trimmed.starts_with('~'))
        {
            let c0 = trimmed.chars().next().unwrap();
            let len = trimmed.chars().take_while(|&c| c == c0).count();
            (len >= 3, c0)
        } else {
            (false, '\0')
        };
        if is_fence {
            if !in_fence {
                // 开启围栏
                in_fence = true;
                fence_char = fc;
                out.push_str(line);
                continue;
            } else if fc == fence_char {
                // 关闭围栏（同字符且 >=3）
                in_fence = false;
                out.push_str(line);
                continue;
            }
        }
        if in_fence {
            out.push_str(line);
            continue;
        }
        // 普通行：逐字符处理行内代码与星号 run
        let chars: Vec<char> = body.chars().collect();
        let n = chars.len();
        let mut i = 0;
        while i < n {
            let c = chars[i];
            // 行内代码（CommonMark 行内代码不跨行，本行内原样输出）
            if c == '`' {
                out.push(c);
                i += 1;
                while i < n && chars[i] != '`' {
                    out.push(chars[i]);
                    i += 1;
                }
                if i < n {
                    out.push('`');
                    i += 1;
                }
                continue;
            }
            if c == '*' {
                let mut cnt = 0;
                while i + cnt < n && chars[i + cnt] == '*' {
                    cnt += 1;
                }
                star_seq += 1;
                let prev = if i > 0 { chars[i - 1] } else { '\0' };
                let next = if i + cnt < n { chars[i + cnt] } else { '\0' };
                let prev_has_text = prev != '\0' && prev.is_alphanumeric();
                let next_has_text = next != '\0' && next.is_alphanumeric();
                let prev_not_ws = prev != '\0' && !prev.is_whitespace();
                let next_not_ws = next != '\0' && !next.is_whitespace();
                if cnt >= 2 {
                    if star_seq % 2 == 1 {
                        // opener：前面紧贴文字且后面非空白 → 前面补空格（使其可作 opener）
                        if prev_has_text && next_not_ws {
                            out.push(' ');
                        }
                        for _ in 0..cnt {
                            out.push('*');
                        }
                    } else {
                        // closer：后面紧贴文字且前面非空白 → 后面补空格（使其可作 closer）
                        for _ in 0..cnt {
                            out.push('*');
                        }
                        if next_has_text && prev_not_ws {
                            out.push(' ');
                        }
                    }
                } else {
                    // 单星号 run 只参与配对计数（决定后续 `**` 的奇偶），不改动内容
                    out.push(c);
                }
                i += cnt;
                continue;
            }
            out.push(c);
            i += 1;
        }
        if line.ends_with('\n') {
            out.push('\n');
        }
    }
    out
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

    #[test]
    fn bold_next_to_chinese_renders_not_as_source() {
        // 左边界紧贴汉字（最常见的中文写作习惯）：修复前显示字面 **加粗**
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
        // 独立成词、标点边界、链接内、列表内：原本就能渲染，不误伤
        for md in ["**加粗**", "，**加粗**。", "[**加粗**](url)", "- **加粗**", "## **加粗**"] {
            let html = render(md);
            assert!(html.contains("<strong>加粗</strong>"), "case: {md}");
            assert!(!html.contains("**"), "case: {md}");
        }
    }

    #[test]
    fn bold_fix_skips_code() {
        // 围栏代码块与行内代码内的 * 不得改动
        let md = "```rust\nlet x = a**b**c;\n```\n\n正文 **加粗** `a**b**c` 结束。";
        let html = render(md);
        assert!(html.contains("a**b**c"), "代码内 ** 应原样保留");
        assert!(html.contains("<code>a**b**c</code>"), "行内代码内 ** 应原样保留");
        // 正文中正常的强调不受影响
        assert!(html.contains("<strong>加粗</strong>"));
    }

    #[test]
    fn bold_fix_after_code_block_still_works() {
        // 围栏代码块之后的正文强调同样修复（围栏关闭后配对序号连续）
        let md = "```\ncode **here**\n```\n后续**加粗**文";
        let html = render(md);
        assert!(html.contains("code **here**"), "代码内 ** 应原样保留");
        assert!(!html.contains("后续**"), "正文 ** 不应残留源码");
        assert!(html.contains("<strong>加粗</strong>"), "代码块后的强调应正常渲染");
    }
}
