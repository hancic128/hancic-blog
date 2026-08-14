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

/// 百分号解码（UTF-8）：`%E8%B4%A2` → `财`。仅当字符串含合法 `%XX` 序列时解码，
/// 非法序列原样保留。用于展示层还原历史迁移数据里被 URL 编码的中文文件名。
pub(crate) fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = hex_val(bytes[i + 1]);
            let lo = hex_val(bytes[i + 2]);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push(hi * 16 + lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
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

    #[test]
    fn decodes_percent_utf8() {
        assert_eq!(percent_decode("%E8%B4%A2%E5%AF%8C%E5%88%86%E5%B1%82-Kdpk.png"), "财富分层-Kdpk.png");
        // 普通中文原样保留
        assert_eq!(percent_decode("身体得分.jpeg"), "身体得分.jpeg");
        // 非法序列原样保留
        assert_eq!(percent_decode("100%有效%ZZ"), "100%有效%ZZ");
        assert_eq!(percent_decode("100%"), "100%");
    }
}
