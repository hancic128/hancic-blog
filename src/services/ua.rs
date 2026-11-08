//! User-Agent 解析：设备类型 / 浏览器 / 操作系统 / 爬虫识别。
//!
//! 只做「够用就好」的规则匹配，不引入 `ua-parser` 之类的重依赖：博客后台的
//! 访问分析只需要粗粒度的构成占比，规则表清晰可维护、可测试。
//!
//! 判定顺序有讲究——**先认爬虫**，再认平台，最后认浏览器：
//! - 爬虫 UA 常同时含 `Mozilla` 与 `Safari`（伪装成浏览器），若先判浏览器会
//!   把它误归成 Chrome/Safari；
//! - 微信内置浏览器（MicroMessenger）同时含 `Android/iOS` 与 `Safari`，归到
//!   微信而非「移动端 Safari」更能说明来源；
//! - 平板要**先于**手机判定：iPad 的 UA 含 `iPad` 也含 `Mobile`，Android 平板
//!   的 UA 含 `Android` 但**不含** `Mobile`，两者都不能只看 `Mobile`。
//!
//! 未识别时返回「其他」而非 `unknown`，保证分组后不会出现空桶。

/// 设备类型分组。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    Desktop,
    Mobile,
    Tablet,
    Bot,
    Other,
}

impl DeviceType {
    /// 存库用的稳定字符串（按值排序有意义：桌面 → 移动 → 平板 → 爬虫 → 其他）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Mobile => "mobile",
            Self::Tablet => "tablet",
            Self::Bot => "bot",
            Self::Other => "other",
        }
    }
}

/// UA 解析结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserAgentInfo {
    pub device_type: DeviceType,
    pub browser: String,
    pub os: String,
}

impl UserAgentInfo {
    /// 空 UA（无 User-Agent 头，或爬虫未声明）：归为「其他」，不伪装成桌面。
    fn empty() -> Self {
        Self {
            device_type: DeviceType::Other,
            browser: "其他".into(),
            os: "其他".into(),
        }
    }
}

/// 爬虫 UA 特征串（小写匹配）。覆盖主流搜索引擎爬虫、社交平台预览抓取、
/// 通用监控与 AI 抓取器——这些访问会显著抬高 PV 计数，剔除后真实阅读才准。
const BOT_SIGNATURES: &[&str] = &[
    "bot",
    "crawler",
    "spider",
    "slurp",
    "curl/",
    "wget/",
    "python-requests",
    "python-urllib",
    "go-http-client",
    "okhttp",
    "axios",
    "node-fetch",
    "postmanruntime",
    "headlesschrome",
    "phantomjs",
    "scrapy",
    "zgrab",
    "masscan",
    "nmap",
    "semrush",
    "ahrefs",
    "mj12",
    "dotbot",
    "petalbot",
    "yandex",
    "baiduspider",
    "bingpreview",
    "feedfetcher",
    "mediapartners",
    "adsbot",
    "googlebot",
    "applebot",
    "chatgpt",
    "gptbot",
    "claudebot",
    "perplexitybot",
    "ccbot",
];

/// 判定是否爬虫。命中任一特征串即认定。
pub fn is_bot(ua: &str) -> bool {
    let lower = ua.to_ascii_lowercase();
    if lower.is_empty() {
        return false;
    }
    BOT_SIGNATURES.iter().any(|sig| lower.contains(sig))
}

/// 解析 User-Agent。
///
/// 返回的 `browser` / `os` 为中文展示名（如「Chrome」「Windows」），
/// 便于后台直接展示而不必再维护一张映射表。
pub fn parse(ua: &str) -> UserAgentInfo {
    let ua = ua.trim();
    if ua.is_empty() {
        return UserAgentInfo::empty();
    }
    let lower = ua.to_ascii_lowercase();

    if is_bot(ua) {
        // 爬虫不细分浏览器/系统——归为「爬虫」比硬猜 Chrome 更有信息量。
        return UserAgentInfo {
            device_type: DeviceType::Bot,
            browser: "爬虫".into(),
            os: "爬虫".into(),
        };
    }

    let device_type = detect_device(&lower);
    let browser = detect_browser(&lower);
    let os = detect_os(&lower);
    UserAgentInfo {
        device_type,
        browser,
        os,
    }
}

/// 设备类型判定（入参已转小写）。
fn detect_device(lower: &str) -> DeviceType {
    // 平板优先于手机：iPad UA 含 Mobile，Android 平板不含 Mobile 但含 Tablet
    if lower.contains("ipad")
        || lower.contains("tablet")
        || lower.contains("playbook")
        || lower.contains("kindle")
        || lower.contains("silk/")
        || (lower.contains("android") && !lower.contains("mobile"))
    {
        return DeviceType::Tablet;
    }
    if lower.contains("mobi")
        || lower.contains("iphone")
        || lower.contains("ipod")
        || lower.contains("android")
        || lower.contains("windows phone")
        || lower.contains("blackberry")
        || lower.contains("opera mini")
        || lower.contains("opera mobi")
    {
        return DeviceType::Mobile;
    }
    if lower.contains("windows")
        || lower.contains("macintosh")
        || lower.contains("mac os")
        || lower.contains("linux")
        || lower.contains("x11")
        || lower.contains("cros")
    {
        return DeviceType::Desktop;
    }
    DeviceType::Other
}

/// 浏览器判定（入参已转小写）。
///
/// 顺序：微信/QQ 等内置浏览器 → Edge → Chrome 系 → Firefox → Safari。
/// Chromium 内核的浏览器（Chrome/Edge/Opera/三星等）都含 `chrome`，
/// 故需靠各自更具体的特征串先行命中。
fn detect_browser(lower: &str) -> String {
    if lower.contains("micromessenger") {
        return "微信内置".into();
    }
    if lower.contains("qq/") || lower.contains("mqqbrowser") {
        return "QQ 内置".into();
    }
    if lower.contains("alipayclient") || lower.contains("alipay") {
        return "支付宝".into();
    }
    if lower.contains("baiduboxapp") {
        return "百度".into();
    }
    if lower.contains("edg/") || lower.contains("edge/") || lower.contains("edga/") {
        return "Edge".into();
    }
    if lower.contains("opr/") || lower.contains("opera") {
        return "Opera".into();
    }
    if lower.contains("samsungbrowser") {
        return "三星浏览器".into();
    }
    if lower.contains("ucbrowser") || lower.contains("ucweb") {
        return "UC 浏览器".into();
    }
    if lower.contains("firefox/") || lower.contains("fxios/") {
        return "Firefox".into();
    }
    if lower.contains("chrome/") || lower.contains("crios/") || lower.contains("chromium") {
        return "Chrome".into();
    }
    if lower.contains("safari/") || lower.contains("applewebkit") {
        return "Safari".into();
    }
    if lower.contains("msie ") || lower.contains("trident/") {
        return "IE".into();
    }
    "其他".into()
}

/// 操作系统判定（入参已转小写）。
fn detect_os(lower: &str) -> String {
    // 鸿蒙优先于 Android：HarmonyOS UA 通常同时含 Android 与 Harmony/Android
    // 兼容标识，若先判 Android 会把鸿蒙设备全归成 Android。
    if lower.contains("harmony") || lower.contains("openharmony") {
        return "鸿蒙".into();
    }
    if lower.contains("windows nt") || lower.contains("windows phone") {
        return "Windows".into();
    }
    if lower.contains("android") {
        return "Android".into();
    }
    if lower.contains("iphone") || lower.contains("ipad") || lower.contains("ipod") {
        return "iOS".into();
    }
    if lower.contains("mac os") || lower.contains("macintosh") {
        return "macOS".into();
    }
    if lower.contains("linux") || lower.contains("x11") {
        return "Linux".into();
    }
    "其他".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    const IPHONE_SAFARI: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";
    const WINDOWS_CHROME: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
    const IPAD: &str = "Mozilla/5.0 (iPad; CPU OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";
    const ANDROID_TABLET: &str = "Mozilla/5.0 (Linux; Android 13; SM-X700) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
    const ANDROID_PHONE: &str = "Mozilla/5.0 (Linux; Android 13; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36";
    const WECHAT: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148 MicroMessenger/8.0.42";
    const GOOGLEBOT: &str =
        "Mozilla/5.0 (compatible; Googlebot/2.1; +http://www.google.com/bot.html)";

    #[test]
    fn empty_ua_is_other() {
        let info = parse("");
        assert_eq!(info.device_type, DeviceType::Other);
        assert_eq!(info.browser, "其他");
        assert_eq!(info.os, "其他");
    }

    #[test]
    fn desktop_windows_chrome() {
        let info = parse(WINDOWS_CHROME);
        assert_eq!(info.device_type, DeviceType::Desktop);
        assert_eq!(info.browser, "Chrome");
        assert_eq!(info.os, "Windows");
    }

    #[test]
    fn mobile_iphone_safari() {
        let info = parse(IPHONE_SAFARI);
        assert_eq!(info.device_type, DeviceType::Mobile);
        assert_eq!(info.browser, "Safari");
        assert_eq!(info.os, "iOS");
    }

    #[test]
    fn ipad_counts_as_tablet_not_mobile() {
        // iPad UA 同时含 iPad 与 Mobile——平板必须先判
        let info = parse(IPAD);
        assert_eq!(info.device_type, DeviceType::Tablet);
        assert_eq!(info.os, "iOS");
    }

    #[test]
    fn android_tablet_without_mobile_is_tablet() {
        let info = parse(ANDROID_TABLET);
        assert_eq!(info.device_type, DeviceType::Tablet);
        assert_eq!(info.os, "Android");
    }

    #[test]
    fn android_phone_is_mobile() {
        let info = parse(ANDROID_PHONE);
        assert_eq!(info.device_type, DeviceType::Mobile);
    }

    #[test]
    fn wechat_embedded_browser_not_safari() {
        let info = parse(WECHAT);
        assert_eq!(info.browser, "微信内置");
        assert_eq!(info.device_type, DeviceType::Mobile);
    }

    #[test]
    fn bots_detected_and_not_treated_as_chrome() {
        // 爬虫 UA 含 Safari/Mozilla，若先判浏览器会误归 Chrome
        let info = parse(GOOGLEBOT);
        assert_eq!(info.device_type, DeviceType::Bot);
        assert_eq!(info.browser, "爬虫");
        assert!(is_bot(GOOGLEBOT));
    }

    #[test]
    fn common_bots_detected() {
        for ua in [
            "Mozilla/5.0 (compatible; Baiduspider/2.0; +http://www.baidu.com/search/spider.html)",
            "curl/8.4.0",
            "python-requests/2.31.0",
            "Mozilla/5.0 (compatible; YandexBot/3.0)",
        ] {
            assert!(is_bot(ua), "应识别为爬虫: {ua}");
        }
    }

    #[test]
    fn regular_ua_is_not_bot() {
        assert!(!is_bot(WINDOWS_CHROME));
        assert!(!is_bot(IPHONE_SAFARI));
        assert!(!is_bot(""));
    }

    #[test]
    fn edge_and_firefox_recognized() {
        let edge = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36 Edg/120.0.0.0";
        assert_eq!(parse(edge).browser, "Edge");
        let firefox = "Mozilla/5.0 (X11; Linux x86_64; rv:121.0) Gecko/20100101 Firefox/121.0";
        let info = parse(firefox);
        assert_eq!(info.browser, "Firefox");
        assert_eq!(info.os, "Linux");
        assert_eq!(info.device_type, DeviceType::Desktop);
    }

    #[test]
    fn device_type_as_str_is_stable() {
        assert_eq!(DeviceType::Desktop.as_str(), "desktop");
        assert_eq!(DeviceType::Mobile.as_str(), "mobile");
        assert_eq!(DeviceType::Tablet.as_str(), "tablet");
        assert_eq!(DeviceType::Bot.as_str(), "bot");
        assert_eq!(DeviceType::Other.as_str(), "other");
    }
}
