//! 通用小工具。

/// HTML 文本转义：`&`/`<`/`>`/`"`/`'` → 实体（防止用户内容注入 HTML）。
///
/// 用于需要 `| safe` 直出的场景（FTS snippet 高亮、兜底错误页），
/// 与 tera 自动转义覆盖的字符集保持一致。
pub(crate) fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_html_specials() {
        assert_eq!(
            html_escape(r#"<script>alert("x") & 'y'</script>"#),
            "&lt;script&gt;alert(&quot;x&quot;) &amp; &#x27;y&#x27;&lt;/script&gt;"
        );
        assert_eq!(html_escape("普通文本 <mark>高亮</mark>"), "普通文本 &lt;mark&gt;高亮&lt;/mark&gt;");
    }
}
