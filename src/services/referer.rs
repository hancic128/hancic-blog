//! Referer 解析：来源域名 / 来源路径 / 搜索引擎关键词。
//!
//! `page_views.referer` 存的是完整 Referer 头（可能很长、可能带追踪参数），
//! 但后台分析真正需要的是三件事：来源站点是谁、指向了对方站哪一页、
//! 以及「读者当时在搜什么关键词」。这里在**写入时**一次性解析成可聚合的
//! 字段，读侧就能直接 `GROUP BY`，不必在 SQL 里做字符串魔法。
//!
//! 不引入 `url` crate：Referer 的解析需求很窄（取 host、取 path、抽 query
//! 参数），手写更可控，也避免为一个小功能增加依赖树。

/// 解析出的 Referer 结构。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RefererInfo {
    /// 来源域名（小写，不含端口；无 referer 时为空）。
    pub domain: String,
    /// 来源路径（含查询串，截断到合理长度；无 referer 时为空）。
    pub path: String,
    /// 搜索引擎关键词（非搜索引擎来源时为空）。
    pub search_query: String,
    /// 是否为搜索引擎来源。
    pub is_search_engine: bool,
}

/// 来源路径最大保留长度：过长的路径（含追踪参数）对分析无价值，
/// 截断可避免表格被超长字符串撑开。
pub const MAX_PATH_LEN: usize = 200;

/// 搜索引擎域名 → 查询参数名。主流搜索引擎的关键词参数名各不相同，
/// 逐个列出比「见到 q= 就当关键词」更可靠——后者会把普通页面的
/// `?q=` 参数误判成搜索词。
const SEARCH_ENGINES: &[(&str, &[&str])] = &[
    ("google.", &["q"]),
    ("bing.com", &["q"]),
    ("baidu.com", &["wd", "word"]),
    ("so.com", &["q"]),
    ("sogou.com", &["query"]),
    ("duckduckgo.com", &["q"]),
    ("yandex.", &["text"]),
    ("yahoo.", &["p"]),
    ("ecosia.org", &["q"]),
    ("startpage.com", &["query"]),
    ("searx.", &["q"]),
    ("search.brave.com", &["q"]),
    ("cn.bing.com", &["q"]),
    ("m.baidu.com", &["wd", "word"]),
];

/// 解析 Referer 头。
pub fn parse(referer: &str) -> RefererInfo {
    let referer = referer.trim();
    if referer.is_empty() {
        return RefererInfo::default();
    }
    // 去掉协议头：`https://host/path` → `host/path`
    let rest = referer
        .strip_prefix("https://")
        .or_else(|| referer.strip_prefix("http://"))
        .unwrap_or(referer);
    // 拆出 host 与 path
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    // 去掉端口与用户信息
    let host = host
        .rsplit('@')
        .next()
        .unwrap_or(host)
        .split(':')
        .next()
        .unwrap_or(host)
        .to_ascii_lowercase();

    let (search_query, is_search_engine) = extract_search_query(&host, path);

    RefererInfo {
        domain: host,
        path: truncate_path(path),
        search_query,
        is_search_engine,
    }
}

/// 截断过长的路径（按字符边界，避免切碎 UTF-8 多字节字符）。
fn truncate_path(path: &str) -> String {
    if path.chars().count() <= MAX_PATH_LEN {
        return path.to_string();
    }
    let mut out: String = path.chars().take(MAX_PATH_LEN).collect();
    out.push('…');
    out
}

/// 从路径的查询串里抽取搜索引擎关键词。
///
/// 返回 `(关键词, 是否搜索引擎来源)`。命中搜索引擎域名但参数缺失时，
/// 关键词为空但仍标记为搜索引擎来源——这样「搜索来源有多少次访问」
/// 不会因为某次没带 q 参数就丢失。
fn extract_search_query(host: &str, path: &str) -> (String, bool) {
    for (engine, params) in SEARCH_ENGINES {
        if !host.contains(engine) {
            continue;
        }
        let Some(query) = path.split_once('?').map(|(_, q)| q) else {
            return (String::new(), true);
        };
        for param in *params {
            if let Some(value) = query_param(query, param) {
                let decoded = percent_decode(&value);
                let trimmed = decoded.trim();
                if !trimmed.is_empty() {
                    return (trimmed.chars().take(80).collect(), true);
                }
            }
        }
        return (String::new(), true);
    }
    (String::new(), false)
}

/// 从查询串中取出指定参数的值（原始百分号编码形态）。
fn query_param(query: &str, name: &str) -> Option<String> {
    for pair in query.split('&') {
        // 首个参数可能带 `?` 前缀（query 已从 `?` 后切出，一般不会有，
        // 但空片段如 `&&` 会产生空 pair，这里跳过）
        let pair = pair.strip_prefix('?').unwrap_or(pair);
        if pair.is_empty() {
            continue;
        }
        let Some((k, v)) = pair.split_once('=') else {
            continue;
        };
        if k.eq_ignore_ascii_case(name) {
            return Some(v.to_string());
        }
    }
    None
}

/// 百分号解码：`%XX` → 字节；`+` → 空格（表单编码惯例）。
///
/// 非法的 `%` 序列原样保留（不报错），避免一条脏 referer 打断整个统计。
pub fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => match u8::from_str_radix(&input[i + 1..i + 3], 16) {
                Ok(b) => {
                    out.push(b);
                    i += 3;
                }
                Err(_) => {
                    out.push(b'%');
                    i += 1;
                }
            },
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_referer_is_blank() {
        let info = parse("");
        assert_eq!(info.domain, "");
        assert_eq!(info.path, "");
        assert_eq!(info.search_query, "");
        assert!(!info.is_search_engine);
    }

    #[test]
    fn extracts_domain_and_path() {
        let info = parse("https://mp.weixin.qq.com/s/abc123");
        assert_eq!(info.domain, "mp.weixin.qq.com");
        assert_eq!(info.path, "/s/abc123");
    }

    #[test]
    fn strips_port_and_scheme() {
        let info = parse("http://example.com:8080/page");
        assert_eq!(info.domain, "example.com");
        assert_eq!(info.path, "/page");
    }

    #[test]
    fn domain_without_path() {
        let info = parse("https://example.com");
        assert_eq!(info.domain, "example.com");
        assert_eq!(info.path, "");
    }

    #[test]
    fn google_query_extracted() {
        let info = parse("https://www.google.com/search?q=rust+%E6%95%99%E7%A8%8B&hl=zh-CN");
        assert_eq!(info.domain, "www.google.com");
        assert_eq!(info.search_query, "rust 教程");
        assert!(info.is_search_engine);
    }

    #[test]
    fn baidu_query_extracted() {
        let info = parse("https://www.baidu.com/s?wd=%E7%A8%8B%E5%BA%8F%E8%AE%BE%E8%AE%A1");
        assert_eq!(info.search_query, "程序设计");
        assert!(info.is_search_engine);
    }

    #[test]
    fn search_engine_without_keyword_still_flagged() {
        let info = parse("https://www.google.com/");
        assert!(info.is_search_engine);
        assert_eq!(info.search_query, "");
    }

    #[test]
    fn non_search_site_with_q_param_is_not_search() {
        // 普通站点的 ?q= 参数不应被误判成搜索词
        let info = parse("https://example.com/list?q=hello");
        assert!(!info.is_search_engine);
        assert_eq!(info.search_query, "");
    }

    #[test]
    fn long_path_truncated() {
        let long = format!("https://example.com/{}", "a".repeat(500));
        let info = parse(&long);
        assert!(info.path.chars().count() <= MAX_PATH_LEN + 1);
        assert!(info.path.ends_with('…'));
    }

    #[test]
    fn percent_decode_handles_invalid_sequences() {
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("a+b"), "a b");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }

    #[test]
    fn utf8_keyword_decodes_correctly() {
        let info = parse("https://www.baidu.com/s?wd=%E5%8D%95%E6%96%87%E4%BB%B6");
        assert_eq!(info.search_query, "单文件");
    }
}
