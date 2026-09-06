//! 站点时区：读取 `settings.timezone`（IANA 名，系统设置页配置）并解析为
//! `chrono_tz::Tz`。缺失/解析失败回退默认时区 Asia/Shanghai。
//!
//! 前后台按天分组展示（说说按天、仪表盘阅读/点赞趋势）统一走这里，
//! 避免各模块各自实现导致语义漂移。

use crate::db::Db;
use crate::services::settings;
use std::str::FromStr;

/// 默认时区：settings.timezone 缺失或非法时的回退值。
pub const DEFAULT_TZ: &str = "Asia/Shanghai";

/// 读取 settings.timezone 并解析为 `chrono_tz::Tz`；缺失/解析失败回退默认时区。
pub async fn site_timezone(db: &Db) -> chrono_tz::Tz {
    let raw = settings::get(db, "timezone").await.ok().flatten();
    raw.as_deref()
        .and_then(|s| chrono_tz::Tz::from_str(s).ok())
        .unwrap_or_else(|| {
            chrono_tz::Tz::from_str(DEFAULT_TZ).expect("默认时区 Asia/Shanghai 应合法")
        })
}

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;

/// 站点时区下日期边界的 UTC 时刻：from → 该日 00:00（本地）的 UTC；
/// to → 次日 00:00（本地）的 UTC（开区间上界）。非法日期视为不设限。
pub fn local_day_utc_bounds(
    from: Option<&str>,
    to: Option<&str>,
    tz: &Tz,
) -> (Option<String>, Option<String>) {
    let day_start_utc = |d: NaiveDate| -> String {
        let ndt = d.and_hms_opt(0, 0, 0).expect("00:00:00 为合法时刻");
        let local = tz
            .from_local_datetime(&ndt)
            .earliest()
            .unwrap_or_else(|| DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc).with_timezone(tz));
        local
            .with_timezone(&Utc)
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string()
    };
    let lower = from
        .filter(|f| !f.is_empty())
        .and_then(|f| NaiveDate::parse_from_str(f, "%Y-%m-%d").ok())
        .map(day_start_utc);
    let upper = to
        .filter(|t| !t.is_empty())
        .and_then(|t| NaiveDate::parse_from_str(t, "%Y-%m-%d").ok())
        .and_then(|d| d.succ_opt())
        .map(day_start_utc);
    (lower, upper)
}
