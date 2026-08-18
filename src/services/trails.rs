//! 徒步轨迹服务：GPX 解析、统计、Douglas-Peucker 抽稀与 CRUD。
//!
//! 导入流程（`import_gpx`）：
//! 1. `quick-xml` 解析 GPX（`<trkseg>` 下 `<trkpt>` 的 lat/lon/ele/time）；
//! 2. 计算统计（haversine 里程、累计爬升/下降、运动时长、均速、海拔范围）；
//! 3. 抽稀（约 30m 阈值）得简化坐标 JSON 存库（总览地图用）；
//! 4. GPX 原文件存 `data/trails/{uuid}.gpx`，完整坐标 JSON 存 `data/trails/{id}.json`
//!    （详情页直接加载，避免每次解析 GPX）。
//!
//! 容错：无 `<trkpt>` 或点数 <2 视为无效轨迹，拒绝入库（BadRequest）。

use crate::db::Db;
use crate::error::AppError;
use crate::models::Trail;
use chrono::{DateTime, Utc};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::Path;
use uuid::Uuid;

/// Douglas-Peucker 抽稀阈值（米）：几千点压到几百点，端点恒保留。
pub const SIMPLIFY_THRESHOLD_M: f64 = 30.0;

/// 地球平均半径（米，haversine 与抽稀投影距离用）。
const EARTH_RADIUS_M: f64 = 6_371_000.0;

/// 单轨迹点（GPX `<trkpt>`）。
#[derive(Debug, Clone, Copy)]
pub struct TrailPoint {
    pub lat: f64,
    pub lon: f64,
    pub ele: Option<f64>,
    pub time: Option<DateTime<Utc>>,
    /// 瞬时速度（km/h，导入时按相邻点距离/时间差计算）
    pub speed: f64,
}

/// 解析 + 统计 + 抽稀结果（导入时计算一次，其余持久化在 trails 表与 JSON 文件）。
#[derive(Debug, Clone)]
pub struct TrailData {
    pub points: Vec<TrailPoint>,
    /// GPX 内轨迹名（`<trk><name>`，缺省 None）
    pub name: Option<String>,
    pub distance_m: f64,
    pub elevation_gain_m: f64,
    pub elevation_loss_m: f64,
    pub moving_seconds: i64,
    pub avg_speed_kmh: f64,
    pub max_elevation_m: Option<f64>,
    pub min_elevation_m: Option<f64>,
    pub started_at: Option<DateTime<Utc>>,
    /// 抽稀后坐标（lat, lon）
    pub simplified: Vec<(f64, f64, f64)>,
}

// ---------- GPX 解析 ----------

/// 解析 GPX：收集全部 `<trkpt>`（跨多个 `<trkseg>`/`<trk>`）与轨迹名。
/// 返回（轨迹点, 名称）；无有效 `<trkpt>` 时返回空 vec（由调用方拒绝入库）。
pub fn parse_gpx(data: &[u8]) -> Result<(Vec<TrailPoint>, Option<String>), String> {
    let mut reader = Reader::from_reader(data);
    reader.config_mut().trim_text(true);
    let mut points: Vec<TrailPoint> = Vec::new();
    let mut trk_name: Option<String> = None; // <trk><name>（优先）
    let mut meta_name: Option<String> = None; // <metadata><name>（兜底）
    let mut in_metadata = false;
    let mut in_author = false;
    let mut in_trk = false;
    let mut in_trkpt = false;
    let mut in_trk_name = false;
    let mut in_meta_name = false;
    let mut cur_lat = 0.0f64;
    let mut cur_lon = 0.0f64;
    let mut in_ele = false;
    let mut in_time = false;
    let mut ele_buf = String::new();
    let mut time_buf = String::new();
    let mut name_buf = String::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Err(e) => return Err(format!("XML 解析失败: {e}")),
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => match e.name().as_ref() {
                b"metadata" => in_metadata = true,
                b"author" => in_author = true,
                b"name" if in_metadata && !in_author => {
                    in_meta_name = true;
                    name_buf.clear();
                }
                b"trk" => {
                    in_trk = true;
                    in_trk_name = false;
                }
                b"name" if in_trk => {
                    in_trk_name = true;
                    name_buf.clear();
                }
                b"trkpt" => {
                    cur_lat = attr_f64(&e, b"lat")?;
                    cur_lon = attr_f64(&e, b"lon")?;
                    in_trkpt = true;
                    in_ele = false;
                    in_time = false;
                }
                b"ele" if in_trkpt => {
                    in_ele = true;
                    ele_buf.clear();
                }
                b"time" if in_trkpt => {
                    in_time = true;
                    time_buf.clear();
                }
                _ => {}
            },
            Ok(Event::Text(t)) => {
                let text = t.decode().map_err(|e| format!("XML 文本解析失败: {e}"))?;
                if in_ele {
                    ele_buf.push_str(&text);
                }
                if in_time {
                    time_buf.push_str(&text);
                }
                if in_trk_name || in_meta_name {
                    name_buf.push_str(&text);
                }
            }
            Ok(Event::End(e)) => match e.name().as_ref() {
                b"metadata" => in_metadata = false,
                b"author" => in_author = false,
                b"trk" => {
                    in_trk = false;
                    in_trk_name = false;
                }
                b"name" if in_trk_name => {
                    in_trk_name = false;
                    if trk_name.is_none() && !name_buf.trim().is_empty() {
                        trk_name = Some(name_buf.trim().to_string());
                    }
                }
                b"name" if in_meta_name => {
                    in_meta_name = false;
                    if meta_name.is_none() && !name_buf.trim().is_empty() {
                        meta_name = Some(name_buf.trim().to_string());
                    }
                }
                b"trkpt" => {
                    let ele = ele_buf.trim().parse::<f64>().ok();
                    let time = parse_gpx_time(time_buf.trim());
                    points.push(TrailPoint {
                        lat: cur_lat,
                        lon: cur_lon,
                        ele,
                        time,
                        speed: 0.0,
                    });
                    in_trkpt = false;
                    in_ele = false;
                    in_time = false;
                }
                b"ele" => in_ele = false,
                b"time" => in_time = false,
                _ => {}
            },
            _ => {}
        }
    }
    Ok((points, trk_name.or(meta_name)))
}

/// 读取 `<trkpt>` 的 lat/lon 属性（必填，缺失/非法即解析失败）。
fn attr_f64(e: &BytesStart, key: &[u8]) -> Result<f64, String> {
    for attr in e.attributes() {
        let attr = attr.map_err(|err| format!("属性解析失败: {err}"))?;
        if attr.key.as_ref() == key {
            let v = std::str::from_utf8(&attr.value)
                .map_err(|err| format!("属性值解析失败: {err}"))?
                .trim();
            return v
                .parse::<f64>()
                .map_err(|_| format!("属性 {key:?} 非法: {v}"));
        }
    }
    Err(format!("缺少属性 {key:?}"))
}

/// GPX `<time>`（RFC3339，可能带毫秒/时区偏移）→ UTC。
fn parse_gpx_time(s: &str) -> Option<DateTime<Utc>> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

// ---------- 统计计算 ----------

/// haversine 距离（米）。
pub fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let a = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    EARTH_RADIUS_M * c
}

/// 计算统计：距离/爬升/下降/运动时长/均速/海拔范围/起点时间。
///
/// - 累计爬升/下降：相邻点海拔正差/负差累加（任一点缺海拔时跳过该段）；
/// - 运动时长：全部有点时间的最小→最大跨度（个别点缺时间时按可用时间估算）；
///   完全无时间时为 0（均速随之 0）。
pub fn compute_stats(points: &[TrailPoint]) -> TrailData {
    // 计算每点瞬时速度（km/h）：相邻点距离 / 时间差；首点与无时间差为 0
    let mut pts: Vec<TrailPoint> = points.to_vec();
    for i in 1..pts.len() {
        let dt = match (pts[i - 1].time, pts[i].time) {
            (Some(a), Some(b)) => (b - a).num_seconds().max(0) as f64,
            _ => 0.0,
        };
        let dist = haversine_m(pts[i - 1].lat, pts[i - 1].lon, pts[i].lat, pts[i].lon);
        pts[i].speed = if dt > 0.0 { dist / dt * 3.6 } else { 0.0 };
    }
    let mut distance_m = 0.0f64;
    let mut elevation_gain_m = 0.0f64;
    let mut elevation_loss_m = 0.0f64;
    for w in pts.windows(2) {
        distance_m += haversine_m(w[0].lat, w[0].lon, w[1].lat, w[1].lon);
        if let (Some(a), Some(b)) = (w[0].ele, w[1].ele) {
            let d = b - a;
            if d > 0.0 {
                elevation_gain_m += d;
            } else {
                elevation_loss_m += -d;
            }
        }
    }
    let mut max_elevation_m: Option<f64> = None;
    let mut min_elevation_m: Option<f64> = None;
    for p in &pts {
        if let Some(e) = p.ele {
            max_elevation_m = Some(max_elevation_m.map_or(e, |m: f64| m.max(e)));
            min_elevation_m = Some(min_elevation_m.map_or(e, |m: f64| m.min(e)));
        }
    }
    let times: Vec<DateTime<Utc>> = pts.iter().filter_map(|p| p.time).collect();
    let moving_seconds = match (times.iter().min(), times.iter().max()) {
        (Some(min), Some(max)) => max.signed_duration_since(*min).num_seconds().max(0),
        _ => 0,
    };
    let avg_speed_kmh = if moving_seconds > 0 {
        distance_m / (moving_seconds as f64) * 3.6
    } else {
        0.0
    };
    let started_at = times.iter().min().copied();
    let simplified = simplify(&pts);
    TrailData {
        points: pts,
        name: None,
        distance_m,
        elevation_gain_m,
        elevation_loss_m,
        moving_seconds,
        avg_speed_kmh,
        max_elevation_m,
        min_elevation_m,
        started_at,
        simplified,
    }
}

// ---------- Douglas-Peucker 抽稀 ----------

/// 简化轨迹：取 (lat, lon, speed)，DP 抽稀后返回坐标序列。
pub fn simplify(points: &[TrailPoint]) -> Vec<(f64, f64, f64)> {
    let coords: Vec<(f64, f64, f64)> = points.iter().map(|p| (p.lat, p.lon, p.speed)).collect();
    douglas_peucker(&coords, SIMPLIFY_THRESHOLD_M)
}

/// Douglas-Peucker 抽稀（迭代实现，避免递归爆栈）：端点恒保留，
/// 距线段超过阈值的中间点保留并递归细分；结果保持原顺序。
pub fn douglas_peucker(points: &[(f64, f64, f64)], threshold_m: f64) -> Vec<(f64, f64, f64)> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack: Vec<(usize, usize)> = vec![(0, points.len() - 1)];
    while let Some((start, end)) = stack.pop() {
        if end <= start + 1 {
            continue;
        }
        let mut max_d = 0.0f64;
        let mut idx = 0usize;
        for i in (start + 1)..end {
            let d = seg_distance_m(points[start], points[end], points[i]);
            if d > max_d {
                max_d = d;
                idx = i;
            }
        }
        if max_d > threshold_m {
            keep[idx] = true;
            stack.push((start, idx));
            stack.push((idx, end));
        }
    }
    points
        .iter()
        .enumerate()
        .filter(|(i, _)| keep[*i])
        .map(|(_, p)| *p)
        .collect()
}

/// 点到线段距离（米）：以线段中点纬度为基准做等距圆柱投影后算欧氏距离，
/// 再乘地球半径。抽稀场景精度足够，避免逐段 haversine 的复杂度。
fn seg_distance_m(a: (f64, f64, f64), b: (f64, f64, f64), p: (f64, f64, f64)) -> f64 {
    let cos_lat = ((a.0 + b.0) / 2.0).to_radians().cos();
    let to_xy = |pt: (f64, f64, f64)| {
        let lat_r = pt.0.to_radians();
        let lon_r = pt.1.to_radians();
        (lon_r * cos_lat, lat_r)
    };
    let (ax, ay) = to_xy(a);
    let (bx, by) = to_xy(b);
    let (px, py) = to_xy(p);
    let abx = bx - ax;
    let aby = by - ay;
    let len2 = abx * abx + aby * aby;
    if len2 == 0.0 {
        return haversine_m(a.0, a.1, p.0, p.1);
    }
    let t = (((px - ax) * abx + (py - ay) * aby) / len2).clamp(0.0, 1.0);
    let cx = ax + t * abx;
    let cy = ay + t * aby;
    let dx = px - cx;
    let dy = py - cy;
    (dx * dx + dy * dy).sqrt() * EARTH_RADIUS_M
}

/// 坐标序列 → `[[lat,lon,speed],...]` JSON 字符串。
pub fn coords_json(coords: &[(f64, f64, f64)]) -> String {
    json!(coords
        .iter()
        .map(|(lat, lon, speed)| json!([lat, lon, speed]))
        .collect::<Vec<_>>())
    .to_string()
}

// ---------- 导入 ----------

/// 导入 GPX：解析 + 统计 + 抽稀 → 落盘 GPX 与完整坐标 JSON → 插库。
///
/// `trails_dir` 为 `data/trails/`；非法 GPX / 点数 <2 返回 BadRequest 拒绝入库。
pub async fn import_gpx(
    db: &Db,
    trails_dir: &Path,
    name: &str,
    fallback_name: &str,
    description: &str,
    data: &[u8],
) -> Result<Trail, AppError> {
    let (points, gpx_name) =
        parse_gpx(data).map_err(|e| AppError::BadRequest(format!("GPX 解析失败: {e}")))?;
    if points.len() < 2 {
        return Err(AppError::BadRequest(
            "GPX 中有效轨迹点不足（至少 2 个点）".into(),
        ));
    }
    // 内容哈希去重：同一 GPX 文件（字节一致）拒绝重复上传
    let sha256 = {
        let mut h = Sha256::new();
        h.update(data);
        h.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>()
    };
    if let Some(existing) = sqlx::query_as::<_, (String,)>("SELECT name FROM trails WHERE sha256 = ? LIMIT 1")
        .bind(&sha256)
        .fetch_optional(db)
        .await?
    {
        return Err(AppError::BadRequest(format!(
            "已存在相同轨迹「{}」，请勿重复上传",
            existing.0
        )));
    }
    // 名称优先级：表单填写 > GPX <name> > 上传文件名（去 .gpx）> 兜底
    let name = if !name.trim().is_empty() {
        name.trim().to_string()
    } else if let Some(g) = gpx_name.filter(|s| !s.trim().is_empty()) {
        g
    } else if !fallback_name.trim().is_empty() {
        fallback_name.trim().to_string()
    } else {
        "未命名轨迹".to_string()
    };
    let stats = compute_stats(&points);
    let simplified_json = coords_json(&stats.simplified);

    std::fs::create_dir_all(trails_dir).map_err(internal)?;
    let file_name = format!("{}.gpx", Uuid::new_v4());
    std::fs::write(trails_dir.join(&file_name), data).map_err(internal)?;

    // 先插库拿 id，再写完整坐标 JSON（详情页直接加载）。插库失败回滚 GPX 文件。
    let insert = sqlx::query(
        "INSERT INTO trails \
         (name, description, file_path, sha256, started_at, distance_m, elevation_gain_m, \
          elevation_loss_m, moving_seconds, avg_speed_kmh, max_elevation_m, min_elevation_m, \
          start_lat, start_lon, end_lat, end_lon, simplified, point_count) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&name)
    .bind(description)
    .bind(file_name.as_str())
    .bind(sha256.as_str())
    .bind(stats.started_at.map(|t| t.to_rfc3339()))
    .bind(stats.distance_m)
    .bind(stats.elevation_gain_m)
    .bind(stats.elevation_loss_m)
    .bind(stats.moving_seconds)
    .bind(stats.avg_speed_kmh)
    .bind(stats.max_elevation_m)
    .bind(stats.min_elevation_m)
    .bind(stats.points.first().map(|p| p.lat))
    .bind(stats.points.first().map(|p| p.lon))
    .bind(stats.points.last().map(|p| p.lat))
    .bind(stats.points.last().map(|p| p.lon))
    .bind(simplified_json)
    .bind(points.len() as i64)
    .execute(db)
    .await;
    let id = match insert {
        Ok(r) => r.last_insert_rowid(),
        Err(e) => {
            let _ = std::fs::remove_file(trails_dir.join(&file_name));
            return Err(e.into());
        }
    };
    let full = coords_json(&stats.points.iter().map(|p| (p.lat, p.lon, p.speed)).collect::<Vec<_>>());
    if let Err(e) = std::fs::write(trails_dir.join(format!("{id}.json")), &full) {
        // 完整坐标缺失不影响浏览（详情页有 GPX 兜底），仅告警
        tracing::warn!("写入轨迹完整坐标失败 id={id}: {e}");
    }
    get_trail(db, id)
        .await?
        .ok_or_else(|| AppError::Internal("轨迹落库失败".into()))
}

// ---------- CRUD ----------

const TRAIL_COLUMNS: &str = "id, name, description, file_path, started_at, distance_m, \
     elevation_gain_m, elevation_loss_m, moving_seconds, avg_speed_kmh, max_elevation_m, \
     min_elevation_m, start_lat, start_lon, end_lat, end_lon, simplified, point_count, \
     created_at, updated_at";

/// 轨迹列表排序方式（后台可切换；前台总览默认「最近」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrailSort {
    /// 按开始时间倒序（无时间者排最后）
    Recent,
    /// 按开始时间正序
    Oldest,
    /// 按里程倒序
    Distance,
    /// 按累计爬升倒序
    Gain,
    /// 按运动时长倒序
    Duration,
}

impl TrailSort {
    /// 解析 `?sort=` 查询参数，未知值回退「最近」。
    pub fn parse(s: Option<&str>) -> Self {
        match s {
            Some("oldest") => Self::Oldest,
            Some("distance") => Self::Distance,
            Some("gain") => Self::Gain,
            Some("duration") => Self::Duration,
            _ => Self::Recent,
        }
    }

    /// 对应 `ORDER BY` 片段（所有数值列先排 NULL 到末尾）。
    pub fn order_by(self) -> &'static str {
        match self {
            Self::Recent => "started_at IS NULL, started_at DESC, id DESC",
            Self::Oldest => "started_at IS NULL, started_at ASC, id ASC",
            Self::Distance => "distance_m IS NULL, distance_m DESC, id DESC",
            Self::Gain => "elevation_gain_m IS NULL, elevation_gain_m DESC, id DESC",
            Self::Duration => "moving_seconds IS NULL, moving_seconds DESC, id DESC",
        }
    }
}

/// 轨迹列表：按指定排序（默认最近）。
pub async fn list_trails(db: &Db, sort: TrailSort) -> Result<Vec<Trail>, AppError> {
    let rows = sqlx::query_as::<_, Trail>(&format!(
        "SELECT {TRAIL_COLUMNS} FROM trails ORDER BY {}",
        sort.order_by()
    ))
    .fetch_all(db)
    .await?;
    Ok(rows)
}

pub async fn get_trail(db: &Db, id: i64) -> Result<Option<Trail>, AppError> {
    let row = sqlx::query_as::<_, Trail>(&format!(
        "SELECT {TRAIL_COLUMNS} FROM trails WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(db)
    .await?;
    Ok(row)
}

/// 改名称/描述（坐标与统计不可改，需重新上传）。
pub async fn update_trail(db: &Db, id: i64, name: &str, description: &str) -> Result<Trail, AppError> {
    let r = sqlx::query(
        "UPDATE trails SET name = ?, description = ?, \
         updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?",
    )
    .bind(name)
    .bind(description)
    .bind(id)
    .execute(db)
    .await?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound("轨迹不存在".into()));
    }
    get_trail(db, id)
        .await?
        .ok_or_else(|| AppError::Internal("轨迹读取失败".into()))
}

/// 删除轨迹：删 DB 行 + 磁盘 GPX 原文件 + 完整坐标 JSON（不存在视为成功）。
pub async fn delete_trail(db: &Db, trails_dir: &Path, id: i64) -> Result<(), AppError> {
    let Some(trail) = get_trail(db, id).await? else {
        return Ok(());
    };
    let gpx = trails_dir.join(&trail.file_path);
    if gpx.exists() {
        let _ = std::fs::remove_file(&gpx);
    }
    let full = trails_dir.join(format!("{id}.json"));
    if full.exists() {
        let _ = std::fs::remove_file(&full);
    }
    sqlx::query("DELETE FROM trails WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

// ---------- 完整坐标加载（详情页） ----------

/// 读 `data/trails/{id}.json` 的完整坐标（导入时落盘）；缺失/损坏返回 None。
pub fn load_full_coords(trails_dir: &Path, id: i64) -> Option<Vec<(f64, f64, f64)>> {
    let text = std::fs::read_to_string(trails_dir.join(format!("{id}.json"))).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let arr = value.as_array()?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let pair = item.as_array()?;
        out.push((
            pair.first()?.as_f64()?,
            pair.get(1)?.as_f64()?,
            pair.get(2).and_then(serde_json::Value::as_f64).unwrap_or(0.0),
        ));
    }
    Some(out)
}

/// 详情页兜底：完整坐标 JSON 缺失时解析 GPX 原文件重算（正常导入不会走到）。
pub fn load_gpx_coords(trails_dir: &Path, file_path: &str) -> Option<Vec<(f64, f64, f64)>> {
    let data = std::fs::read(trails_dir.join(file_path)).ok()?;
    let (points, _) = parse_gpx(&data).ok()?;
    if points.len() < 2 {
        return None;
    }
    let stats = compute_stats(&points);
    Some(stats.points.iter().map(|p| (p.lat, p.lon, p.speed)).collect())
}

fn internal(e: impl std::fmt::Display) -> AppError {
    AppError::Internal(e.to_string())
}

/// 秒 → 人类可读运动时长（"3 小时 25 分"，不足 1 分钟显示"<1 分"）。
/// 后台列表与前台详情共用。
pub fn format_moving(secs: Option<i64>) -> String {
    let secs = secs.unwrap_or(0).max(0);
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    if h > 0 {
        format!("{h}h{m}m")
    } else if m > 0 {
        format!("{m}m")
    } else {
        "<1m".into()
    }
}

// ---------- 单元测试 ----------

#[cfg(test)]
mod tests {
    use super::*;

    const GPX_SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1" creator="两步路">
  <metadata><name>元数据名</name></metadata>
  <trk>
    <name>测试轨迹</name>
    <trkseg>
      <trkpt lat="31.2304" lon="121.4737"><ele>100</ele><time>2026-01-01T00:00:00Z</time></trkpt>
      <trkpt lat="31.2394" lon="121.4737"><ele>150</ele><time>2026-01-01T00:01:00Z</time></trkpt>
      <trkpt lat="31.2394" lon="121.4827"><ele>100</ele><time>2026-01-01T00:02:00Z</time></trkpt>
    </trkseg>
  </trk>
</gpx>"#;

    #[test]
    fn parse_gpx_extracts_points_name_and_times() {
        let (points, name) = parse_gpx(GPX_SAMPLE.as_bytes()).unwrap();
        assert_eq!(points.len(), 3);
        assert_eq!(name.as_deref(), Some("测试轨迹"));
        assert!((points[0].lat - 31.2304).abs() < 1e-9);
        assert!((points[1].ele.unwrap() - 150.0).abs() < 1e-9);
        assert_eq!(
            points[0].time.map(|t| t.to_rfc3339()),
            Some("2026-01-01T00:00:00+00:00".into())
        );
    }

    #[test]
    fn parse_gpx_rejects_missing_lat() {
        let gpx = r#"<gpx><trk><trkseg>
          <trkpt lon="121"><ele>1</ele></trkpt>
          <trkpt lat="31" lon="121"><ele>2</ele></trkpt>
        </trkseg></trk></gpx>"#;
        assert!(parse_gpx(gpx.as_bytes()).is_err());
    }

    #[test]
    fn parse_gpx_without_trkpt_returns_empty() {
        let gpx = r#"<gpx><trk><name>x</name></trk></gpx>"#;
        let (points, name) = parse_gpx(gpx.as_bytes()).unwrap();
        assert!(points.is_empty());
        assert_eq!(name.as_deref(), Some("x"));
    }

    #[test]
    fn parse_gpx_falls_back_to_metadata_name() {
        let gpx = r#"<gpx><metadata><name>元数据轨迹</name></metadata><trk><trkseg>
          <trkpt lat="31" lon="121"><ele>1</ele></trkpt>
          <trkpt lat="31.01" lon="121"><ele>2</ele></trkpt>
        </trkseg></trk></gpx>"#;
        let (points, name) = parse_gpx(gpx.as_bytes()).unwrap();
        assert_eq!(points.len(), 2);
        assert_eq!(name.as_deref(), Some("元数据轨迹"));
    }

    #[test]
    fn compute_stats_calculates_distance_gain_loss_and_time() {
        let (points, _) = parse_gpx(GPX_SAMPLE.as_bytes()).unwrap();
        let s = compute_stats(&points);
        // 期望距离：逐段 haversine 累加（测试自算，与实现同公式但独立验证数量级）
        let expected = haversine_m(31.2304, 121.4737, 31.2394, 121.4737)
            + haversine_m(31.2394, 121.4737, 31.2394, 121.4827);
        assert!(
            (s.distance_m - expected).abs() < 1e-6,
            "distance={} expected={expected}",
            s.distance_m
        );
        assert!((s.elevation_gain_m - 50.0).abs() < 1e-9);
        assert!((s.elevation_loss_m - 50.0).abs() < 1e-9);
        assert_eq!(s.moving_seconds, 120);
        assert!((s.avg_speed_kmh - s.distance_m / 120.0 * 3.6).abs() < 1e-9);
        assert_eq!(s.max_elevation_m, Some(150.0));
        assert_eq!(s.min_elevation_m, Some(100.0));
        assert_eq!(
            s.started_at.map(|t| t.to_rfc3339()),
            Some("2026-01-01T00:00:00+00:00".into())
        );
    }

    #[test]
    fn compute_stats_handles_missing_elevation_and_time() {
        let gpx = r#"<gpx><trk><trkseg>
          <trkpt lat="31" lon="121"><ele>100</ele></trkpt>
          <trkpt lat="31.001" lon="121"><time>2026-01-01T00:00:00Z</time></trkpt>
          <trkpt lat="31.002" lon="121"><ele>200</ele></trkpt>
        </trkseg></trk></gpx>"#;
        let (points, _) = parse_gpx(gpx.as_bytes()).unwrap();
        let s = compute_stats(&points);
        // 中间段缺海拔：跳过；仅一段有海拔差（100→200 爬升 100）
        assert!((s.elevation_gain_m - 100.0).abs() < 1e-9);
        assert_eq!(s.elevation_loss_m, 0.0);
        // 仅一个点有时间：运动时长 0
        assert_eq!(s.moving_seconds, 0);
        assert_eq!(s.avg_speed_kmh, 0.0);
    }

    #[test]
    fn douglas_peucker_reduces_straight_line_to_endpoints() {
        let pts: Vec<(f64, f64, f64)> = (0..100)
            .map(|i| (30.0, 120.0 + i as f64 * 0.001, 0.0))
            .collect();
        let out = douglas_peucker(&pts, 30.0);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], pts[0]);
        assert_eq!(out[1], pts[99]);
    }

    #[test]
    fn douglas_peucker_keeps_deviation_point() {
        // 99 个点直线 + 中间一个突出 100m（0.001° ≈ 111m 经向偏 0.0009°）
        let mut pts: Vec<(f64, f64, f64)> = Vec::new();
        for i in 0..50 {
            pts.push((30.0, 120.0 + i as f64 * 0.001, 0.0));
        }
        pts.push((30.0009, 120.0 + 50.0 * 0.001, 0.0)); // 突出 ~100m
        for i in 51..100 {
            pts.push((30.0, 120.0 + i as f64 * 0.001, 0.0));
        }
        let out = douglas_peucker(&pts, 30.0);
        assert!(out.len() >= 3, "凸点应保留，len={}", out.len());
        assert!(out.contains(&pts[50]));
        assert_eq!(out.first(), Some(&pts[0]));
        assert_eq!(out.last(), Some(&pts[99]));
    }

    #[test]
    fn douglas_peucker_keeps_small_tracks_intact() {
        let pts = vec![(30.0, 120.0, 0.0), (30.001, 120.0, 0.0)];
        let out = douglas_peucker(&pts, 30.0);
        assert_eq!(out, pts);
    }

    #[test]
    fn coords_json_roundtrips() {
        let coords = vec![(31.0, 121.0, 2.5), (31.1, 121.2, 0.0)];
        let s = coords_json(&coords);
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v[0][0].as_f64(), Some(31.0));
        assert_eq!(v[1][1].as_f64(), Some(121.2));
        assert_eq!(v[0][2].as_f64(), Some(2.5));
    }
}
