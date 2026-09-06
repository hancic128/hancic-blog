//! 后台统计：工具函数 + 清空日志。
//!
//! 统计内容（卡片 + 趋势 + 文章排行 Top10 + 地区分布 + 跳转来源）已合并进
//! 仪表盘（/admin），本模块仅保留清空日志 handler 与仪表盘复用的区间/聚合工具。
//!
//! 路由：
//!   POST /admin/stats/clear   清空阅读明细日志（前端二次确认）
//!
//! 时间语义：`from`/`to` 与趋势分组均按站点时区（settings.timezone，默认
//! Asia/Shanghai）的自然日；快捷天数窗口以本地今天为末日在服务层换算边界。
//!
//! 鉴权约定同其他后台模块：GET 未登录 302 跳登录；POST 先 `require_admin`
//! 再过 CSRF。

use crate::error::AppError;
use crate::services::stats;
use crate::{session, AppState};
use axum::extract::{Form, State};
use axum::response::Response;
use chrono::{Days, NaiveDate, Utc};
use serde_json::{Value, json};
use std::collections::HashMap;
use tower_sessions::Session;

// ---------- 清理日志 ----------

pub async fn clear(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<HashMap<String, String>>,
) -> Result<Response, AppError> {
    session::require_admin(&session).await?;
    session::verify_csrf(&session, form.get("csrf").map(String::as_str)).await?;
    match stats::clear_logs(&state.db).await {
        Ok(()) => Ok(super::redirect(&state.config.base_path, "/admin")),
        Err(e) => {
            tracing::error!("清理阅读日志失败: {e:?}");
            Ok(super::redirect(&state.config.base_path, "/admin"))
        }
    }
}

// ---------- 参数与数据加工（仪表盘复用） ----------

/// 快捷天数参数：`days=30/60/90`（默认 30，上限 3650），供模板快捷按钮回显。
pub fn query_days(query: &HashMap<String, String>) -> u32 {
    query
        .get("days")
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(30)
        .clamp(1, 3650)
}

/// 解析 from/to（`YYYY-MM-DD`）：两者缺省 → 近 30 天（含今天）；
/// 支持 `days=30/60/90` 快捷参数（仅当 from/to 都缺省时生效，默认 30）；
/// 显式 `range=all` → 返回 (None, None) 表示「全部」不设限（服务层无过滤）；
/// 提供但非法 → Err；只给一侧时另一侧保持 None（服务层视为不设限）。
pub fn parse_range(
    query: &HashMap<String, String>,
    tz: &chrono_tz::Tz,
) -> Result<(Option<String>, Option<String>), String> {
    let from = query
        .get("from")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let to = query
        .get("to")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    for (name, v) in [("from", &from), ("to", &to)] {
        if let Some(v) = v {
            if NaiveDate::parse_from_str(v, "%Y-%m-%d").is_err() {
                return Err(format!("{name} 参数必须是 YYYY-MM-DD"));
            }
        }
    }
    if let (Some(f), Some(t)) = (&from, &to) {
        let f = NaiveDate::parse_from_str(f, "%Y-%m-%d").expect("已校验");
        let t = NaiveDate::parse_from_str(t, "%Y-%m-%d").expect("已校验");
        if f > t {
            return Err("from 不能晚于 to".into());
        }
    }
    if from.is_none() && to.is_none() {
        // 显式「全部」（?range=all）：返回 (None, None)，服务层视为不设限；
        // 否则按快捷天数 days=30/60/90（默认 30），与「近 30 天」缺省行为一致
        if query.get("range").is_some_and(|v| v == "all") {
            return Ok((None, None));
        }
        let days = query_days(query);
        let today = Utc::now().with_timezone(tz).date_naive();
        let from_d = today
            .checked_sub_days(Days::new(u64::from(days - 1)))
            .expect("days 上限内不会下溢");
        return Ok((
            Some(from_d.format("%Y-%m-%d").to_string()),
            Some(today.format("%Y-%m-%d").to_string()),
        ));
    }
    Ok((from, to))
}

/// 有效区间（缺省一侧补近 30 天，末日本地今天），用于表单回填与趋势横轴。
pub fn effective_range(
    from: &Option<String>,
    to: &Option<String>,
    tz: &chrono_tz::Tz,
) -> (String, String) {
    let today = Utc::now().with_timezone(tz).date_naive();
    let from_str = from.clone().unwrap_or_else(|| {
        today
            .checked_sub_days(Days::new(29))
            .expect("30 天内日期不会下溢")
            .format("%Y-%m-%d")
            .to_string()
    });
    let to_str = to.clone().unwrap_or_else(|| today.format("%Y-%m-%d").to_string());
    (from_str, to_str)
}

/// `from` 到 `to`（含端点）的日期序列，升序；两端均已校验为 `YYYY-MM-DD`。
pub fn date_range(from: &str, to: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut d = NaiveDate::parse_from_str(from, "%Y-%m-%d").expect("已校验");
    let end = NaiveDate::parse_from_str(to, "%Y-%m-%d").expect("已校验");
    while d <= end {
        out.push(d.format("%Y-%m-%d").to_string());
        d = d.succ_opt().expect("日期递增不会溢出");
    }
    out
}

/// 地区明细：按 (国家, 省份) 分组计数，阅读降序（城市并入省份，不单独展示）。
/// 空维度保持原样由模板显示「—」。
pub fn region_view(rows: &[stats::RegionStat]) -> Vec<Value> {
    let mut groups: Vec<(String, String, i64)> = Vec::new();
    for r in rows {
        let key = (r.country.as_str(), r.province.as_str());
        match groups
            .iter_mut()
            .find(|(c, p, _)| (c.as_str(), p.as_str()) == key)
        {
            Some((_, _, count)) => *count += r.count,
            None => groups.push((r.country.clone(), r.province.clone(), r.count)),
        }
    }
    groups.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
    groups
        .into_iter()
        .map(|(country, province, count)| {
            json!({ "country": country, "province": province, "count": count })
        })
        .collect()
}

/// 跳转来源中文名映射（key 见 `services::stats::classify_referer`）。
pub fn source_label(source: &str) -> &'static str {
    match source {
        "direct" => "直接访问",
        "wechat" => "微信公众号",
        "zhihu" => "知乎",
        "csdn" => "CSDN",
        "juejin" => "掘金",
        "weibo" => "微博",
        "jianshu" => "简书",
        "github" => "GitHub",
        "google" => "Google 搜索",
        "bing" => "Bing 搜索",
        "baidu" => "百度搜索",
        _ => "其他",
    }
}

/// 跳转来源视图：key + 中文名 + 计数，阅读降序。
pub fn source_view(rows: &[stats::SourceStat]) -> Vec<Value> {
    rows.iter()
        .map(|r| {
            json!({ "key": r.source, "label": source_label(&r.source), "count": r.count })
        })
        .collect()
}
